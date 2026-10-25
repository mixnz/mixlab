//! What the front end is asked to serve: whether it would accept a configuration, the addresses
//! that wake a stopped service, and the documents and fragments it renders.

use std::collections::BTreeMap;

use mixengine_proto::{FrontEndServer, ServiceId};

use crate::Result;

use super::{
    Document, FrontEndAddition, Generator, Prepared, Role, Served, Upstream, document, recipe,
};

impl Generator {
    /// Would this home's front end accept the configuration installing `pending` would give it?
    ///
    /// Renders the whole home with `pending`'s `[[recipe.front_end]]` fragments in it and shows the
    /// front end's own binary the result, through
    /// [`document::judge`] — which stages, runs the checker and installs nothing.
    /// A fragment the server will not parse is therefore refused **before** anything is downloaded,
    /// unpacked or written, and the home is byte-identical afterwards either way.
    ///
    /// **What is judged is the fragment with the ports the manifest asked for**, not the ports it
    /// will hold: allocation happens when the rows are written, which is after this. A port number
    /// is a token in both configuration languages and cannot change whether one parses, so the
    /// judgement carries over — what does not carry over is the claim that the server saw the exact
    /// bytes that will be installed. It saw the exact *shape*.
    ///
    /// **A home with no front end judges nothing and passes.** There is no binary to ask. The
    /// fragment is judged the first time a front end renders, which is when one is installed.
    ///
    /// # Errors
    ///
    /// [`ServiceError::ConfigRejected`] carrying what the front end said, and whatever rendering
    /// this home's declarations costs.
    ///
    /// [`ServiceError::ConfigRejected`]: super::ServiceError::ConfigRejected
    pub async fn would_serve(&self, pending: &crate::extensions::store::Installed) -> Result<()> {
        // An extension that adds nothing to the front end changes nothing about its configuration,
        // and judging it would put a process launch into every `extension.install` — including
        // every one of the kinds that could not carry a fragment if they wanted to.
        if pending
            .manifest
            .recipe
            .as_ref()
            .is_none_or(|recipe| recipe.front_end.is_empty())
        {
            return Ok(());
        }

        let (prepared, served) = self.declarations_with(Some(pending)).await?;

        for one in &prepared {
            if !matches!(one.recipe.role(), Role::FrontEnd(_)) {
                continue;
            }

            document::judge(
                &one.context.etc,
                &Self::documents(one, &served)?,
                one.recipe.validator(&one.context).as_ref(),
            )
            .await?;
        }

        Ok(())
    }

    /// Every service something can start by connecting to it, and the pair of addresses that takes.
    ///
    /// The activator's address first and the service's own second, which is the order they are used
    /// in: the daemon holds the first and dials the second once the service is up. A service whose
    /// recipe has no activator is not in the map at all.
    ///
    /// **Computed from the same rows and the same contexts a render uses**, so the address the
    /// daemon binds and the address a site file names cannot disagree — which is the failure this
    /// method exists to make impossible, because what it looks like is a site that 502s and an
    /// activator sitting on an address nothing dials.
    ///
    /// # Errors
    ///
    /// Whatever preparing a row costs, and whatever computing either address costs — a home too
    /// deeply nested for the derived socket path, most of all.
    pub async fn activators(&self) -> Result<BTreeMap<ServiceId, (Upstream, Upstream)>> {
        // Through `declarations` rather than a query of its own: the addresses have to be the ones
        // a render would compute, and a second row query here would be a second chance to compute
        // them from something slightly different.
        let (prepared, _served) = self.declarations().await?;

        let mut activators = BTreeMap::new();

        for one in prepared {
            let Some(listen) = one.recipe.upstream(&one.context)? else {
                continue;
            };

            if let Some(activator) = one.recipe.activator(&one.context)? {
                activators.insert(one.context.service.clone(), (activator, listen));
            }
        }

        Ok(activators)
    }

    /// Every address in this home that a connection may start a *stopped* service at.
    ///
    /// **Through `declarations` for [`activators`](Self::activators)' reason**: the addresses have
    /// to be the ones a render computed, and a second query here would be a second chance to
    /// compute them from something slightly different — which is exactly what would happen to a
    /// socket path, whose value is a function of how deep this home is.
    ///
    /// **A service with nothing to hold is absent rather than present and empty.** The caller
    /// iterates this map, and an empty entry is a service it would log about having done nothing
    /// for.
    ///
    /// # Errors
    ///
    /// Whatever preparing every service's context costs, and whatever a recipe reports about an
    /// address it cannot compute.
    pub async fn held_while_stopped(&self) -> Result<BTreeMap<ServiceId, Vec<Upstream>>> {
        let (prepared, _served) = self.declarations().await?;

        let mut held = BTreeMap::new();

        for one in prepared {
            let addresses = one.recipe.held_while_stopped(&one.context)?;

            if !addresses.is_empty() {
                held.insert(one.context.service.clone(), addresses);
            }
        }

        Ok(held)
    }

    /// Every file this service has, rendered.
    ///
    /// **One definition for both callers.** [`install`](Self::install) writes these and
    /// [`drift`](Self::drift) compares them; a second assembly of the set would be a second answer
    /// to "what does this service render to", and the front end's site files are exactly where two
    /// answers would diverge.
    pub(super) fn documents(prepared: &Prepared, served: &[Served]) -> Result<Vec<Document>> {
        let mut documents = recipe::render(prepared.recipe.as_ref(), &prepared.context)?;

        // **Appended to the recipe's own set, not installed by a path of its own.** The checker
        // judges a staging directory, so a site file written anywhere else would be invisible to
        // `caddy validate` and present at run time.
        if matches!(prepared.recipe.role(), Role::FrontEnd(_)) {
            documents.extend(prepared.recipe.sites(&prepared.context, served)?);

            // **And what this home's extensions added.** Under the same condition and appended to
            // the same set, for the same reason: a fragment installed by a path of its own would be
            // invisible to the checker and present at run time.
            documents.extend(prepared.recipe.fragments(&prepared.context)?);
        }

        Ok(documents)
    }

    /// Every installed extension's `[[recipe.front_end]]`, rendered.
    ///
    /// **Here rather than in a recipe**, because the placeholders in a fragment are the
    /// *extension's*: `{install_dir}` means the directory that extension was installed into, and
    /// only its row knows where that is. The server each one names travels beside it so that
    /// [`prepare`](Self::prepare) can give a front end its own and no others.
    ///
    /// # Errors
    ///
    /// [`ExtensionError::ExtensionField`] for a fragment whose placeholders cannot be rendered,
    /// naming `recipe.front_end[<n>]` — a manifest that was installed and can no longer be applied
    /// is reported rather than skipped, which is the rule `php_ini` already follows.
    ///
    /// [`ExtensionError::ExtensionField`]: crate::ExtensionError::ExtensionField
    pub(super) fn fragments<'a>(
        installed: impl Iterator<Item = &'a crate::extensions::store::Installed>,
    ) -> Result<Vec<(FrontEndServer, FrontEndAddition)>> {
        let mut fragments = Vec::new();

        for one in installed {
            let Some(recipe) = &one.manifest.recipe else {
                continue;
            };

            let context = crate::extensions::render::Context::installed(one);

            for (index, entry) in recipe.front_end.iter().enumerate() {
                let field = format!("recipe.front_end[{index}]");

                fragments.push((
                    entry.server,
                    FrontEndAddition {
                        extension: one.id.as_str().to_owned(),
                        fragment: crate::extensions::render::fragment(
                            &one.id,
                            &field,
                            &entry.fragment,
                            &context,
                            entry.server.into(),
                        )?,
                    },
                ));
            }
        }

        Ok(fragments)
    }
}
