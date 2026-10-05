import { useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";

import Button from "../../../../components/Button";
import Modal, { ModalBody, ModalErrors } from "../../../../components/Modal";
import { errorMessage } from "../../../../core/errors";
import { useTranslation } from "../../../../i18n";
import * as api from "../../api";
import type { BlueprintApplied } from "@mixengine/api";
import { describePlanAction, failedSteps } from "../../blueprintPlan";
import { siteUrl } from "../../siteState";
import ElevationDialog from "../ElevationDialog";
import styles from "./AfterApply.module.css";

/** How far along the three-step chain we are. */
type Phase =
  | { kind: "checking" }
  | { kind: "granting"; pending: unknown[]; canPrompt: boolean; reason?: string | null }
  | { kind: "starting" }
  | { kind: "ready"; url: string | null };

interface Props {
  /**
   * The result of the apply that just finished.
   *
   * **A whole result rather than just the project name** — T125. This block used to take a single
   * string, so it had no way to know which step the apply had just failed and said "ready" for
   * every apply whose job did not throw. A `[scaffold]` with a non-zero exit is a
   * `StepResult::Failed` *inside* a successful job (`api/apply.rs` on purpose: a broken
   * post-install script is not worth tearing down the whole project), so "job done" and "apply
   * fine" are two different statements and only the second is worth inviting people to click into
   * the site.
   */
  applied: BlueprintApplied;

  /** The user closes this block. `url` is the address found, or `null` if there is no site. */
  onFinished: (url: string | null) => void;
}

/**
 * What follows a successful `blueprint.apply`: ask for rights → start → open the site.
 *
 * **Three calls, in exactly that order, and the order is the hard part.** `blueprint.apply` never
 * raises the rights prompt — it queues the hosts entry and the certificate, and the client is where
 * that single prompt is spent. Starting *before* the grant would serve the site on a name this
 * machine does not resolve yet, with a certificate no store trusts yet; opening a browser on that
 * is a red error at the end of a green progress bar.
 *
 * An empty queue (the machine already has the name in hosts) goes straight on to starting — there
 * is nothing to ask. Failing to read `elevation.status` also goes on: the site still runs, and
 * standing still because a side question could not be asked is worse than a running site whose
 * name does not resolve yet.
 *
 * **"Start" means what this project needs** — T125, and the wording says exactly that. That set is
 * still not the client's: `service.start` takes a `project` scope, and the daemon is what reads
 * `site_service_links`, the php-fpm pool the site names, and the home's front end. Before T125 this
 * sent an empty target — *every service this home declares* — so bringing up a Laravel site on a
 * machine with four PHP versions and three databases started all eight.
 *
 * **An apply with a failed step must not invite people to click into the site.** A `[scaffold]`
 * with a non-zero exit arrives here inside a *successful* job, so this block used to say "ready"
 * above a half-built directory. The chain still runs in full — the rights queue is still worth
 * spending, the front end still worth starting, and abandoning both just to protest a broken script
 * leaves the machine half done — but what it *says* changes: a different title, the failed steps on
 * top, and the address as a line of text rather than an inviting button.
 *
 * **One component rather than two copies.** This chain used to live only in `QuickStart`, so an
 * apply from the Blueprints screen ended at a list of steps and a Close button: services not
 * started, domain not resolved, and no way to the site just built.
 *
 * **Not nested inside `ApplyDialog`.** Both `ElevationDialog` and this block are `Modal`s, and
 * `Modal` listens for Escape at `window` level: with two stacked, one Escape closes both. So
 * `ApplyDialog` closes first, then the screen that called it brings this component up in its place.
 *
 * **Offer the address, do not navigate by itself.** The first version opened the browser right
 * away once the init command had run; after trying it, this popup turned out to do enough to *show
 * people the result*, and pulling a browser window up in front of someone is a side effect they did
 * not ask for. The button is here; the click is theirs.
 */
export default function AfterApply({ applied, onFinished }: Props) {
  const project = applied.project;
  const failed = failedSteps(applied);
  const [phase, setPhase] = useState<Phase>({ kind: "checking" });
  const [error, setError] = useState("");
  const { t } = useTranslation();

  // The rights pass. Runs exactly once, as soon as this block comes up.
  useEffect(() => {
    if (phase.kind !== "checking") return;
    let live = true;
    api
      .elevationStatus()
      .then((waiting) => {
        if (!live) return;
        if (waiting.pending.length > 0) {
          setPhase({
            kind: "granting",
            pending: waiting.pending,
            canPrompt: waiting.can_prompt,
            reason: waiting.reason,
          });
        } else {
          setPhase({ kind: "starting" });
        }
      })
      // Go on: see the doc above.
      .catch(() => live && setPhase({ kind: "starting" }));
    return () => {
      live = false;
    };
  }, [phase.kind]);

  // The start pass, then finding the address of the site just built.
  useEffect(() => {
    if (phase.kind !== "starting") return;
    let live = true;
    void (async () => {
      try {
        await api.serviceStartProject(project);
        const listed = await api.sites(project);
        const made = listed.sites[0];
        const url = made === undefined ? null : siteUrl(made);
        if (!live) return;
        setPhase({ kind: "ready", url });
      } catch (e) {
        if (!live) return;
        setError(errorMessage(t, e));
        setPhase({ kind: "ready", url: null });
      }
    })();
    return () => {
      live = false;
    };
  }, [phase.kind, project, t]);

  // Closing the rights dialog means going on, not cancelling: `elevation.drop` is not here, so
  // closing only hides it and the queue remains — the Dashboard still counts it. The site should
  // still be started: an unresolved name is the queue's business, not a reason to run nothing.
  if (phase.kind === "granting") {
    return (
      <ElevationDialog
        pending={phase.pending}
        canPrompt={phase.canPrompt}
        reason={phase.reason}
        onClose={() => setPhase({ kind: "starting" })}
      />
    );
  }

  const done = phase.kind === "ready";

  // The title says exactly which state it is in. A heading stuck at "Bringing the project up" above
  // a "ready" line is two sentences arguing in the same dialog — and the heading is what people
  // read first.
  const heading = !done
    ? t("mixengine.afterApply.titleWorking")
    : failed.length > 0
      ? t("mixengine.afterApply.titleTrouble")
      : t("mixengine.afterApply.titleReady");

  return (
    <Modal
      title={heading}
      onClose={() => onFinished(done ? phase.url : null)}
      locked={!done}
      size="small"
      actions={[
        {
          kind: "cancel",
          label: t("mixengine.afterApply.close"),
          onClick: () => onFinished(done ? phase.url : null),
          disabled: !done,
        },
      ]}
    >
      {() => (
        <>
          <ModalBody>
            {!done && (
              <div className={styles.status}>
                <p>{t("mixengine.afterApply.starting")}</p>
                <progress />
              </div>
            )}

            {/* **Above** the address, not below it. The failed step is what decides what people do
                next, and a warning block under a green button is a block nobody reads. */}
            {done && failed.length > 0 && (
              <div className={styles.trouble} role="alert">
                <p>{t("mixengine.afterApply.troubleTitle")}</p>
                <ul>
                  {failed.map((outcome, i) => (
                    <li key={i}>
                      {describePlanAction(t, outcome.action)}
                      {outcome.result.result === "failed" && ` — ${outcome.result.why}`}
                    </li>
                  ))}
                </ul>
              </div>
            )}

            {done && (
              <div className={styles.status}>
                {phase.url === null ? (
                  <p>{t("mixengine.afterApply.noSite")}</p>
                ) : failed.length > 0 ? (
                  // **An address, not an invitation.** The site is real and being served, so hiding
                  // it would hide what people need once they have fixed things; but a primary "Open
                  // website" button under a failed init command is this dialog praising a job it
                  // has just said failed.
                  <p>{t("mixengine.afterApply.readyWithTrouble", { url: phase.url })}</p>
                ) : (
                  <>
                    <p>{t("mixengine.afterApply.ready", { url: phase.url })}</p>
                    <Button variant="primary" data-demo="open-site" onClick={() => void openUrl(phase.url ?? "")}>
                      {t("mixengine.afterApply.open", { url: phase.url })}
                    </Button>
                  </>
                )}
              </div>
            )}
          </ModalBody>
          <ModalErrors messages={[error]} />
        </>
      )}
    </Modal>
  );
}
