//! What this home declares, row by row, and what a service's settings and its drift on disk are,
//! without rendering anything.

use std::collections::BTreeMap;

use mixengine_proto::ServiceId;

use crate::{Error, Result};

use super::{
    Generated, Generator, Prepared, Row, Served, ServiceDrift, Settings, Upstreams, document,
    served,
};

impl Generator {
    /// What this home declares, prepared but not yet written anywhere.
    ///
    /// **The step both [`declared`](Self::declared) and [`drift`](Self::drift) start from**, and the
    /// reason it is one function: the two ask the same question of the same rows and differ only in
    /// what they do with the answer. Two walks would be two definitions of "what this home
    /// declares", and the front end — whose configuration is a function of every site and of where
    /// every pool listens — is where they would come apart.
    ///
    /// **All or nothing.** One row that cannot be prepared fails the call rather than being left out
    /// of the answer, because a service that silently disappears from `mix service list` is a service
    /// somebody goes looking for in the wrong place. What they get instead is a message naming the
    /// row and what is wrong with it.
    ///
    /// # Errors
    ///
    /// [`Error::Database`] when the rows cannot be read; and per row, whatever
    /// [`generate`](Self::generate) reports.
    pub(super) async fn declarations(&self) -> Result<(Vec<Prepared>, Vec<Served>)> {
        self.declarations_with(None).await
    }

    /// [`declarations`](Self::declarations), with one extension that is not installed yet counted as
    /// though it were — roadmap task **T81c**.
    ///
    /// **The only caller is [`would_serve`](Self::would_serve)**, and the parameter exists because
    /// the question it asks cannot be asked any other way: *would this home's front end accept the
    /// configuration it is about to be given?* is a question about a set the table does not hold.
    /// Writing the row first and rolling it back would put the answer after the change it is
    /// supposed to prevent.
    ///
    /// `pending` contributes fragments and nothing else. It has no `services` row, so it runs
    /// nothing and is served nothing; the row it will get is written later, by the install this
    /// judgement is standing in front of.
    pub(super) async fn declarations_with(
        &self,
        pending: Option<&crate::extensions::store::Installed>,
    ) -> Result<(Vec<Prepared>, Vec<Served>)> {
        let rows = sqlx::query_as!(
            Row,
            r#"SELECT s.id                    AS "id!: String",
                      s.instance_name         AS "instance_name!: String",
                      s.port                  AS "port: i64",
                      s.bind_addr             AS "bind_addr!: String",
                      s.data_dir              AS "data_dir: String",
                      s.config_overrides_json AS "overrides!: String",
                      s.limits_json           AS "limits!: String",
                      s.idle_minutes          AS "idle_minutes: i64",
                      s.activation_port       AS "activation_port: i64",
                      p.name                  AS "package: String",
                      p.version               AS "package_version: String",
                      p.install_path          AS "package_path: String",
                      p.provides_json         AS "package_provides: String",
                      r.kind                  AS "runtime: String",
                      r.version               AS "runtime_version: String",
                      r.install_path          AS "runtime_path: String",
                      r.provides_json         AS "runtime_provides: String",
                      s.extension_id          AS "extension: String"
               FROM services s
               LEFT JOIN packages p         ON p.id = s.package_id
               LEFT JOIN runtime_installs r ON r.id = s.runtime_install_id
               ORDER BY s.id"#
        )
        .fetch_all(self.store.pool())
        .await
        .map_err(|source| self.store.failure("read", source))?;

        // **Two passes, and the first one writes nothing.** A site's configuration names the address
        // its pool listens on, and only the pool's own recipe knows what that is — so every context
        // has to be built before any file is rendered. The pass costs a `Context` per row out of a
        // row already fetched.
        let mut prepared = Vec::with_capacity(rows.len());

        // **Read once for the whole walk** — roadmap task T81. An extension's recipe is built out
        // of its row, and a join per service would fetch the same manifests again for every one of
        // them. A home has a handful of extensions and none at all is the common case, so this is
        // one query answering nothing most of the time.
        let mut installed: BTreeMap<String, crate::extensions::store::Installed> =
            crate::extensions::store::all(&self.store)
                .await?
                .into_iter()
                .map(|one| (one.id.as_str().to_owned(), one))
                .collect();

        if let Some(pending) = pending {
            installed.insert(pending.id.as_str().to_owned(), pending.clone());
        }

        // **Rendered once for the whole walk, beside the manifests it comes out of** — roadmap task
        // **T81c**. A fragment's placeholders are the extension's, so the substitution belongs where
        // the extension's row is; what a front-end recipe is handed is text.
        let fragments = Self::fragments(installed.values())?;

        // **Which pools carry a credential, read once for the whole walk** — roadmap task **T82a**,
        // that design's D4. Beside the manifests and the fragments for their reason: answering it
        // reads the extension sites, their frozen database links and each database's recipe, and a
        // join per service would ask those questions once per row.
        let credentials = crate::extensions::pools::credentials(&self.store).await?;

        // **Which home this is, read once for the whole walk** — roadmap task **T126**, and beside
        // the two above for their reason. Every spec that carries an `EnvValue::Keyring` names an
        // address this is the first segment of, so a walk that asked per row would ask the same
        // question of the same table once per service for ever.
        let home = crate::home::id(&self.store).await?;

        // **Whether the home saves resources, read once for the whole walk** — roadmap task
        // **T167b**. It decides the idle policy of every row nobody set, so it is one answer for
        // the walk rather than one read per row.
        let saving = crate::services::save_resources::get(&self.store).await?;

        for row in rows {
            prepared.push(self.prepare(
                row,
                &home,
                &installed,
                &fragments,
                &credentials,
                saving,
            )?);
        }

        let mut upstreams = BTreeMap::new();

        for one in &prepared {
            if let Some(listen) = one.recipe.upstream(&one.context)? {
                // Asked only of a service something points at, so a recipe with no upstream is never
                // asked for an activator either — T70. The pair is inserted together, which is what
                // makes it impossible for a site to name one service's pool and another's activator.
                let activator = one.recipe.activator(&one.context)?;

                upstreams.insert(one.context.service.clone(), Upstreams { listen, activator });
            }
        }

        let served =
            served::served(&self.store, &upstreams, self.paths.certs(), &installed).await?;

        Ok((prepared, served))
    }

    /// Every service this home declares, configured and ready to run.
    ///
    /// **The whole set**, because the caller's next move is a
    /// [`ServiceGraph`](crate::services::ServiceGraph): dependencies, cycles and start order are
    /// properties of a set.
    ///
    /// **[`Generated`] and not [`ServiceSpec`]**, because the specification is only half of what a
    /// walk needs to know. The other half is whether anything on disk moved — [`Generated::changed`]
    /// — which is what turns "the configuration is up to date" into "the process reading it has been
    /// told", and it is knowledge only this call has: by the time a spec is in a caller's hands the
    /// file has already been written and a second look at it would compare a rendering with itself.
    ///
    /// [`drift`](Self::drift) is the read-only half, and answers what this would change.
    ///
    /// # Errors
    ///
    /// [`Error::Database`] when the rows cannot be read; per row, whatever
    /// [`generate`](Self::generate) reports; and per row, whatever installing it reports.
    ///
    /// [`ServiceSpec`]: super::ServiceSpec
    pub async fn declared(&self) -> Result<Vec<Generated>> {
        let (prepared, served) = self.declarations().await?;
        let mut generated = Vec::with_capacity(prepared.len());

        for one in prepared {
            generated.push(self.install(one, &served).await?);
        }

        Ok(generated)
    }

    /// One service's settings, merged, without rendering or installing anything.
    ///
    /// **Read-only, and that is the whole reason it exists** — roadmap task **T53**.
    /// [`generate`](Self::generate) goes through [`declared`](Self::declared), which installs; a
    /// caller that only wants a number would rewrite this home's configuration to get it, and can
    /// reload a running server as a side effect of asking a question. This takes
    /// [`drift`](Self::drift)'s door for the reason stated there — rendering is pure, and nothing
    /// before it touches a disk.
    ///
    /// **The same merge and not a second one.** What comes back is what a rendering of this service
    /// would be made with, so a caller and a template cannot come to disagree about a value. The
    /// first caller is `mix cert status`, which connects to the front end's `https_port`; the only
    /// other way to learn that number is to read it out of the generated configuration, and
    /// `CLAUDE.md` forbids parsing a generated file back into state.
    ///
    /// # Errors
    ///
    /// [`Error::Database`] when the rows cannot be read, whatever merging the overrides reports,
    /// and [`Error::NotFound`] when nothing in this home declares this service.
    pub async fn settings(&self, service: &ServiceId) -> Result<Settings> {
        let (prepared, _served) = self.declarations().await?;

        prepared
            .into_iter()
            .find(|one| one.context.service() == service)
            .map(|one| one.context.settings().clone())
            .ok_or_else(|| Error::NotFound {
                kind: "service",
                id: service.as_str().to_owned(),
            })
    }

    /// What [`declared`](Self::declared) would change on disk, asked without changing it.
    ///
    /// Roadmap task **T47b**, and the read `mix doctor`'s tenth check makes. Rendering is pure — it
    /// builds [`Document`]s in memory — and the comparison is [`document::drift`], so nothing is
    /// staged, nothing is validated and no directory is created. That is what lets a check whose
    /// whole guarantee is "this writes nothing" ask the question at all.
    ///
    /// **A home with no rows drifts in no way**, and answers an empty list rather than an error.
    ///
    /// # Errors
    ///
    /// [`Error::Database`] when the rows cannot be read; per row, whatever
    /// [`generate`](Self::generate) reports; and [`Error::Io`] when a generated directory cannot
    /// be read.
    ///
    /// [`Document`]: super::Document
    pub async fn drift(&self) -> Result<Vec<ServiceDrift>> {
        let (prepared, served) = self.declarations().await?;
        let mut drifts = Vec::with_capacity(prepared.len());

        for one in &prepared {
            drifts.push(ServiceDrift {
                service: one.context.service.clone(),
                drift: document::drift(
                    &one.context.etc,
                    &Self::documents(one, &served)?,
                    one.recipe.swept(),
                )
                .await?,
            });
        }

        Ok(drifts)
    }
}
