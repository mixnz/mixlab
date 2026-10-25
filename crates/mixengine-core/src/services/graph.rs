//! `depends_on` as a graph: what must be up before what, and in which order things come down.
//!
//! [`ServiceSpec::depends_on`] declares one service's edges; nothing until now has held the whole
//! set at once, and every interesting question needs the whole set. A cycle is the obvious one — no
//! single spec can see `a → b → a`, so [`mixengine_proto::ServiceSpecBuilder::build`] rejects only
//! the one case a spec *can* see, itself — but so are "what else has to start first" and "what
//! breaks if I stop this".
//!
//! **This is `core` and not `proto` or `supervisor`.** `proto` owns the vocabulary a spec is
//! written in ([ADR 0006](../../decisions/0006-servicespec-in-proto-and-secret-free.md)) and gains
//! nothing from a topological sort; the supervisor deliberately has no registry, no loop and no
//! clock, which is what lets the daemon's runner own the timing. What is left is domain logic over
//! a set of declared services, which is this crate.
//!
//! **A plan is tiers, not a list.** Two services that depend on nothing may start at the same time,
//! and a flat order throws that away and has to recompute it later. The daemon's runner walks the
//! tiers one service at a time to begin with; the day M3's ten-second budget wants concurrency, the
//! information is already here and nothing has to be re-derived. [`Plan::flat`] is the sequential
//! reading of the same value.
//!
//! Nothing here spawns, waits or persists. A graph is built from specs, answers questions about
//! them, and is thrown away — which is what makes every rule in it testable without a process.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use mixengine_proto::{ServiceId, ServiceSpec};

/// A set of service specs whose `depends_on` edges are known to form a DAG.
///
/// Built by [`ServiceGraph::new`], which is where the three things a single spec cannot check are
/// checked: that no id is declared twice, that every dependency named is a service that exists, and
/// that the edges do not close a loop. Past that point a graph answers questions and cannot fail on
/// its own account, so a caller holding one never has to handle "what if it is a cycle" again.
#[derive(Debug, Clone)]
pub struct ServiceGraph {
    /// Every service, by id. A `BTreeMap` rather than a `HashMap` because the order this iterates
    /// in is the order tiers come out in, and a start order that differs run to run turns a
    /// reproducible boot into an intermittent one.
    specs: BTreeMap<ServiceId, ServiceSpec>,

    /// The forward edges: for each service, what it names in its `depends_on`.
    ///
    /// A `BTreeSet` and not the declared `Vec`, which is the point of holding it at all: an edge
    /// declared twice has to *be* one edge. Counting `["mariadb", "mariadb"]` as two while the
    /// reverse map — a set — can only ever discharge one of them leaves a service waiting forever,
    /// which reads exactly like a loop and is not one. `ServiceSpecBuilder::build` refuses the
    /// repetition, but a spec deserialised from an `extension.toml` or a row has been through no
    /// builder, and this is the crate that trusts those edges.
    dependencies: BTreeMap<ServiceId, BTreeSet<ServiceId>>,

    /// The reverse edges: for each service, who names it in their `depends_on`.
    ///
    /// Stored rather than derived on demand because both directions are needed on every question
    /// worth asking — starting walks dependencies, stopping walks dependents — and computing this
    /// one from the other means a full scan of every spec each time.
    dependents: BTreeMap<ServiceId, BTreeSet<ServiceId>>,
}

impl ServiceGraph {
    /// Assemble a graph, checking everything the specs could not check individually.
    ///
    /// **This does not stand in for [`mixengine_proto::ServiceSpec::validate`]**, which is a
    /// loader's job and reports against the row or file a bad spec came from. It does not depend on
    /// it either: a spec that never went through one is still assembled into edges that mean what
    /// they say, so a graph is never the thing that mistakes an unvalidated spec for a broken one.
    ///
    /// # Errors
    ///
    /// [`GraphError::Duplicate`] when two specs carry the same id; [`GraphError::UnknownDependency`]
    /// when a spec names a dependency that is not in the set; [`GraphError::Cycle`] when the edges
    /// close a loop, carrying the loop itself rather than the fact of one.
    pub fn new(specs: impl IntoIterator<Item = ServiceSpec>) -> Result<Self, GraphError> {
        let mut by_id: BTreeMap<ServiceId, ServiceSpec> = BTreeMap::new();

        for spec in specs {
            if let Some(existing) = by_id.insert(spec.id().clone(), spec) {
                return Err(GraphError::Duplicate {
                    id: existing.id().clone(),
                });
            }
        }

        // Seeded with an entry per service, so `dependents` is total over the graph's ids and every
        // lookup below is an ordinary `get` rather than a question about whether an id is known.
        let mut dependents: BTreeMap<ServiceId, BTreeSet<ServiceId>> = by_id
            .keys()
            .map(|id| (id.clone(), BTreeSet::new()))
            .collect();
        let mut dependencies: BTreeMap<ServiceId, BTreeSet<ServiceId>> = BTreeMap::new();

        // Every edge is checked before any is walked, so an unknown dependency is reported as
        // itself rather than as a service that mysteriously never gets a tier. Both directions are
        // built here and both are sets, so an edge declared twice is one edge in each.
        for spec in by_id.values() {
            let mut declared: BTreeSet<ServiceId> = BTreeSet::new();

            for dependency in spec.depends_on() {
                let Some(dependents) = dependents.get_mut(dependency) else {
                    return Err(GraphError::UnknownDependency {
                        service: spec.id().clone(),
                        dependency: dependency.clone(),
                    });
                };

                dependents.insert(spec.id().clone());
                declared.insert(dependency.clone());
            }

            dependencies.insert(spec.id().clone(), declared);
        }

        let graph = Self {
            specs: by_id,
            dependencies,
            dependents,
        };

        // Kahn's algorithm consumes exactly the acyclic part of a graph, so anything it leaves
        // behind is in a loop or downstream of one. `find_cycle` pays a second walk — only on the
        // failing path — to say which services those are.
        let everything: BTreeSet<ServiceId> = graph.specs.keys().cloned().collect();
        if graph.layered(&everything, Direction::Dependencies).is_err() {
            return Err(GraphError::Cycle {
                path: graph.find_cycle(),
            });
        }

        Ok(graph)
    }

    /// The spec for one service, or `None` if it is not in this graph.
    #[must_use]
    pub fn spec(&self, id: &ServiceId) -> Option<&ServiceSpec> {
        self.specs.get(id)
    }

    /// How many services the graph holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.specs.len()
    }

    /// Whether the graph holds no services at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.specs.is_empty()
    }

    /// Every service in the graph, in [`ServiceId`] order.
    ///
    /// The order a *listing* wants, which is deliberately not the order either plan gives: a start
    /// order groups by dependency, and reading `service.list` down the page to find one service is
    /// not that question. What a plan is for is walking; this is for showing.
    pub fn ids(&self) -> impl Iterator<Item = &ServiceId> {
        self.specs.keys()
    }

    /// Every service, in an order where nothing is reached before what it depends on.
    ///
    /// What autostart at boot walks.
    #[must_use]
    pub fn start_order(&self) -> Plan {
        self.plan(
            &self.specs.keys().cloned().collect(),
            Direction::Dependencies,
        )
    }

    /// Every service, in an order where nothing is reached before what depends on it.
    ///
    /// What `daemon.shutdown` walks. **Not [`ServiceGraph::start_order`] reversed** — for the whole
    /// set the two do coincide, but they are computed from opposite edges and only
    /// [`ServiceGraph::stop_plan`] gives the right answer for a subset, so deriving one from the
    /// other here would be a coincidence waiting to be relied on.
    #[must_use]
    pub fn stop_order(&self) -> Plan {
        self.plan(&self.specs.keys().cloned().collect(), Direction::Dependents)
    }

    /// What must start, and in what order, for `roots` to be running.
    ///
    /// The plan holds the roots **and everything they transitively depend on**: `mix service start
    /// php-fpm@8.3` on a service that names MariaDB is a request to have MariaDB up, and refusing it
    /// would be technically correct and useless.
    ///
    /// # Errors
    ///
    /// [`GraphError::NoSuchService`] when a root is not in this graph.
    pub fn start_plan<'a>(
        &self,
        roots: impl IntoIterator<Item = &'a ServiceId>,
    ) -> Result<Plan, GraphError> {
        Ok(self.plan(&self.known(roots)?, Direction::Dependencies))
    }

    /// What must stop, and in what order, for `roots` to be stopped.
    ///
    /// The mirror image, and the asymmetry is the point: stopping pulls in everything that
    /// transitively **depends on** the roots, because a site left pointed at a database that is
    /// going away is worse than one told its database is down. They come first in the plan, so no
    /// service is ever left running without what it needs.
    ///
    /// # Errors
    ///
    /// [`GraphError::NoSuchService`] when a root is not in this graph.
    pub fn stop_plan<'a>(
        &self,
        roots: impl IntoIterator<Item = &'a ServiceId>,
    ) -> Result<Plan, GraphError> {
        Ok(self.plan(&self.known(roots)?, Direction::Dependents))
    }

    /// The services `id` names directly in its `depends_on`.
    ///
    /// The graph's edges rather than the spec's list — each dependency once, in [`ServiceId`] order,
    /// the same shape [`ServiceGraph::dependents_of`] answers in. Read the spec for what was
    /// literally written.
    ///
    /// # Errors
    ///
    /// [`GraphError::NoSuchService`] when `id` is not in this graph.
    pub fn dependencies_of(&self, id: &ServiceId) -> Result<&BTreeSet<ServiceId>, GraphError> {
        self.dependencies
            .get(id)
            .ok_or_else(|| GraphError::NoSuchService { id: id.clone() })
    }

    /// The services that name `id` directly in their `depends_on`.
    ///
    /// # Errors
    ///
    /// [`GraphError::NoSuchService`] when `id` is not in this graph.
    pub fn dependents_of(&self, id: &ServiceId) -> Result<&BTreeSet<ServiceId>, GraphError> {
        self.dependents
            .get(id)
            .ok_or_else(|| GraphError::NoSuchService { id: id.clone() })
    }

    /// Everything that transitively depends on `id`, `id` itself excluded.
    ///
    /// **What a failure propagates along.** The daemon's runner asks this when a service fails to
    /// start: every planned service in the answer is one that can now never come up, and goes
    /// straight to [`mixengine_proto::ServiceState::Failed`] with
    /// [`mixengine_proto::StateReason::DependencyFailed`] instead of being spawned against a
    /// dependency that is not there. Ordered, so the same failure reads the same way twice.
    ///
    /// # Errors
    ///
    /// [`GraphError::NoSuchService`] when `id` is not in this graph.
    pub fn blocked_by(&self, id: &ServiceId) -> Result<BTreeSet<ServiceId>, GraphError> {
        let roots = self.known(std::iter::once(id))?;
        let mut blocked = self.reachable(&roots, Direction::Dependents);
        blocked.remove(id);

        Ok(blocked)
    }

    /// Check that every root is a service this graph holds, and collect them.
    fn known<'a>(
        &self,
        roots: impl IntoIterator<Item = &'a ServiceId>,
    ) -> Result<BTreeSet<ServiceId>, GraphError> {
        roots
            .into_iter()
            .map(|id| {
                if self.specs.contains_key(id) {
                    Ok(id.clone())
                } else {
                    Err(GraphError::NoSuchService { id: id.clone() })
                }
            })
            .collect()
    }

    /// A plan over the sub-graph reachable from `roots`, which must already be known ids.
    ///
    /// The layering cannot fail here: a `ServiceGraph` is acyclic by construction, and a sub-graph
    /// of a DAG is a DAG. [`ServiceGraph::new`] is the one caller that has neither guarantee yet,
    /// and it calls [`ServiceGraph::layered`] directly for exactly that reason.
    fn plan(&self, roots: &BTreeSet<ServiceId>, direction: Direction) -> Plan {
        let included = self.reachable(roots, direction);
        let tiers = self
            .layered(&included, direction)
            .expect("a ServiceGraph is acyclic by construction");

        Plan { tiers }
    }

    /// The roots plus everything reachable from them along `direction`.
    fn reachable(&self, roots: &BTreeSet<ServiceId>, direction: Direction) -> BTreeSet<ServiceId> {
        let mut seen: BTreeSet<ServiceId> = roots.clone();
        let mut queue: VecDeque<ServiceId> = roots.iter().cloned().collect();

        while let Some(id) = queue.pop_front() {
            for next in self.edges(&id, direction) {
                if seen.insert(next.clone()) {
                    queue.push_back(next.clone());
                }
            }
        }

        seen
    }

    /// Kahn's algorithm over `included`, emitting one tier per round.
    ///
    /// Taking *every* currently-free service per round rather than one at a time is what produces
    /// the tiers: a service is free once everything it must wait for has been emitted, so the
    /// members of a round have no path between them and cannot constrain each other's order.
    ///
    /// The `direction` is what "wait for" means. Starting waits for dependencies, so tier 0 is the
    /// services that depend on nothing; stopping waits for dependents, so tier 0 is the services
    /// nothing depends on. Both are the same walk with the edges read the other way round.
    ///
    /// # Errors
    ///
    /// [`HasCycle`] when a round comes up empty with services still unemitted — the definition of a
    /// loop, since every one of them is still waiting for another one of them.
    fn layered(
        &self,
        included: &BTreeSet<ServiceId>,
        direction: Direction,
    ) -> Result<Vec<Vec<ServiceId>>, HasCycle> {
        // How many services inside the sub-graph each one is still waiting for.
        let mut waiting_on: BTreeMap<ServiceId, usize> = included
            .iter()
            .map(|id| {
                let count = self
                    .edges(id, direction)
                    .iter()
                    .filter(|next| included.contains(*next))
                    .count();
                (id.clone(), count)
            })
            .collect();

        let mut tiers: Vec<Vec<ServiceId>> = Vec::new();
        let mut emitted = 0_usize;

        loop {
            let tier: Vec<ServiceId> = waiting_on
                .iter()
                .filter(|(_, waiting)| **waiting == 0)
                .map(|(id, _)| id.clone())
                .collect();

            if tier.is_empty() {
                break;
            }

            for id in &tier {
                waiting_on.remove(id);

                // Whoever was waiting on this one is now one step freer, and the edges run the
                // other way to find them: a released dependency frees its dependents, and a
                // released dependent frees what it was holding up. Exactly one decrement per edge
                // counted above, because both directions are sets over the same edges.
                for freed in self.edges(id, direction.reversed()) {
                    if let Some(waiting) = waiting_on.get_mut(freed) {
                        *waiting -= 1;
                    }
                }
            }

            emitted += tier.len();
            tiers.push(tier);
        }

        if emitted == included.len() {
            Ok(tiers)
        } else {
            Err(HasCycle)
        }
    }

    /// One service's neighbours along `direction`, each named once.
    ///
    /// Both maps are total over the graph's ids, so an id from outside simply has no neighbours
    /// rather than being a case every caller has to answer for.
    fn edges(&self, id: &ServiceId, direction: Direction) -> &BTreeSet<ServiceId> {
        /// The answer for an id this graph does not hold.
        static NONE: BTreeSet<ServiceId> = BTreeSet::new();

        let map = match direction {
            Direction::Dependencies => &self.dependencies,
            Direction::Dependents => &self.dependents,
        };

        map.get(id).unwrap_or(&NONE)
    }

    /// Recover one concrete loop, for the error message.
    ///
    /// Run only once [`ServiceGraph::layered`] has proved there is one, because it is a second walk
    /// and the answer is worth it exactly once: "there is a loop among caddy, php-fpm and mariadb"
    /// leaves the user to work out which edge to delete, while `caddy → php-fpm → mariadb → caddy`
    /// names it.
    ///
    /// A depth-first search that keeps its own stack rather than recursing, so a chain longer than
    /// the machine's stack is an error message rather than a crash — these specs can come from an
    /// `extension.toml`.
    fn find_cycle(&self) -> Vec<ServiceId> {
        /// Where the search has got to with a service.
        #[derive(Clone, Copy, PartialEq, Eq)]
        enum Visit {
            /// On the current path: reaching it again closes a loop.
            Open,
            /// Fully explored, and led to no loop.
            Closed,
        }

        let mut seen: BTreeMap<&ServiceId, Visit> = BTreeMap::new();

        for root in self.specs.keys() {
            if seen.contains_key(root) {
                continue;
            }

            // The path so far, each step carrying the dependencies it has not tried yet.
            let mut path: Vec<(&ServiceId, Declared<'_>)> = Vec::new();
            seen.insert(root, Visit::Open);
            path.push((root, self.declared(root)));

            while let Some((id, remaining)) = path.last_mut() {
                let here = *id;

                match remaining.next() {
                    Some(next) => match seen.get(next) {
                        // `next` is somewhere on the current path, so the loop runs from there to
                        // here. Every step between the two is in it, and nothing before.
                        Some(Visit::Open) => {
                            let start = path
                                .iter()
                                .position(|(id, _)| *id == next)
                                .unwrap_or_default();

                            return path[start..].iter().map(|(id, _)| (*id).clone()).collect();
                        }
                        Some(Visit::Closed) => {}
                        None => {
                            seen.insert(next, Visit::Open);
                            path.push((next, self.declared(next)));
                        }
                    },
                    None => {
                        seen.insert(here, Visit::Closed);
                        path.pop();
                    }
                }
            }
        }

        // Not reachable through `new`: `layered` leaves a service unconsumed only when it is
        // waiting on one that is never emitted, and with both edge directions held as sets over the
        // same edges the only way that happens is a real loop. An empty path renders as a loop with
        // no names rather than as a panic, because a thin error message is a better failure than a
        // dead daemon.
        Vec::new()
    }

    /// A service's declared dependencies, as the borrowing iterator [`ServiceGraph::find_cycle`]
    /// parks on its stack.
    fn declared(&self, id: &ServiceId) -> Declared<'_> {
        self.edges(id, Direction::Dependencies).iter()
    }
}

/// One service's dependencies, part-way through being walked.
type Declared<'a> = std::collections::btree_set::Iter<'a, ServiceId>;

/// Which way an edge is being read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Direction {
    /// Towards what a service needs: `depends_on`. Starting walks this way.
    Dependencies,
    /// Towards what needs a service. Stopping walks this way.
    Dependents,
}

impl Direction {
    /// The other way, which is how a freed service finds who was waiting on it.
    const fn reversed(self) -> Self {
        match self {
            Self::Dependencies => Self::Dependents,
            Self::Dependents => Self::Dependencies,
        }
    }
}

/// "There is a loop", before [`ServiceGraph::find_cycle`] has named it.
///
/// Private and deliberately empty: the only caller that can meet it is [`ServiceGraph::new`], which
/// turns it into a [`GraphError::Cycle`] that carries the loop. Everywhere else a graph is already
/// acyclic, so this is the signal that a `Result` is being handled rather than a failure a user ever
/// reads.
#[derive(Debug, PartialEq, Eq)]
struct HasCycle;

/// An ordered set of services to act on: tier by tier, with everything a tier needs already done by
/// the time it is reached.
///
/// **The tiers are kept rather than flattened** because they carry the one thing a flat list
/// destroys: which services have no ordering constraint between them. The daemon walks a plan
/// sequentially through [`Plan::flat`]; the concurrency M3's ten-second budget will eventually want
/// is then a change to the walker, not a recomputation of the plan.
///
/// Within a tier the order is by [`ServiceId`], so the same specs always produce the same plan — a
/// boot order that varies run to run turns one broken dependency into a bug that only reproduces on
/// somebody else's machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    tiers: Vec<Vec<ServiceId>>,
}

impl Plan {
    /// The tiers, in the order they must be acted on. Everything within one may proceed at once.
    #[must_use]
    pub fn tiers(&self) -> &[Vec<ServiceId>] {
        &self.tiers
    }

    /// Every service in the plan, flattened into one valid sequential order.
    pub fn flat(&self) -> impl Iterator<Item = &ServiceId> {
        self.tiers.iter().flatten()
    }

    /// How many services the plan covers.
    #[must_use]
    pub fn len(&self) -> usize {
        self.tiers.iter().map(Vec::len).sum()
    }

    /// Whether the plan asks for nothing at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tiers.is_empty()
    }

    /// Whether a service is one of the plan's.
    #[must_use]
    pub fn contains(&self, id: &ServiceId) -> bool {
        self.flat().any(|planned| planned == id)
    }
}

/// A set of specs that cannot be assembled into a dependency graph, or a question asked of one about
/// a service it does not hold.
///
/// Its own type rather than more variants on [`crate::Error`], and reachable from there through
/// [`crate::Error::Graph`]: these are failures of *declaring* services, they name no path and no
/// database, and the daemon reports them against whatever produced the specs — a package definition,
/// an `extension.toml` — rather than against the machine.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum GraphError {
    /// Two specs claim the same id.
    ///
    /// Not merged: one of them would silently win, and which one would depend on the order they
    /// happened to be loaded in.
    #[error("service `{id}` is declared more than once")]
    Duplicate {
        /// The id claimed twice.
        id: ServiceId,
    },

    /// A spec depends on a service that is not in the set.
    ///
    /// Almost always a typo, or a service deleted while something still named it, and it is refused
    /// rather than ignored: a dependency treated as satisfied because nobody can find it is the
    /// ordering bug that only appears on a slow machine.
    #[error("service `{service}` depends on `{dependency}`, which is not a service here")]
    UnknownDependency {
        /// The spec that declared the edge.
        service: ServiceId,
        /// The dependency it named.
        dependency: ServiceId,
    },

    /// The edges close a loop, so no service in it could ever be started first.
    #[error(
        "these services depend on each other in a loop: {}",
        render_cycle(path)
    )]
    Cycle {
        /// The loop, each service once and in order. The last one depends on the first, which is
        /// what closes it — the repetition is in the rendering, not in the data.
        path: Vec<ServiceId>,
    },

    /// A question was asked about a service the graph does not hold.
    #[error("no such service: `{id}`")]
    NoSuchService {
        /// The id that was asked about.
        id: ServiceId,
    },
}

/// `caddy → php-fpm@8.3 → mariadb@main → caddy`: the loop written out, closed back to its start.
fn render_cycle(path: &[ServiceId]) -> String {
    let Some(first) = path.first() else {
        return "(the loop could not be recovered)".to_owned();
    };

    let mut rendered = String::new();
    for id in path {
        rendered.push_str(id.as_str());
        rendered.push_str(" → ");
    }
    rendered.push_str(first.as_str());

    rendered
}

#[cfg(test)]
mod tests;
