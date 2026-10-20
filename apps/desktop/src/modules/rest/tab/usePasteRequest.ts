import { useEffect, useEffectEvent, useRef, useState } from "react";
import { modalDepth } from "../../../core/shortcuts";
import type { Environment } from "../environments";
import { parsePaste } from "../parsePaste";
import { isBlank } from "../requests";
import { pasteOverBlank, pasteRequest, saveRequest } from "../requestsStore";
import { findSubstitutions, substitute, type Substitution } from "../substitute";
import { replaceQuery } from "../syncUrlParams";
import type { RestRequest } from "../types";

interface PasteRequestOptions {
  /** Whether this REST tab is the one on screen. */
  active: boolean;
  activeRequest: RestRequest | undefined;
  env: Environment | null;
  open: (id: string) => void;
}

/** A paste that the environment has names for, and the request it would become. */
export interface SwapOffer {
  request: RestRequest;
  found: Substitution[];
}

export interface PasteRequest {
  editUrl: (url: string) => void;
  pasteIntoUrl: (text: string) => boolean;
  swap: SwapOffer | null;
  setSwap: (swap: SwapOffer | null) => void;
}

/** A cURL command pasted into the URL box or into an empty workspace, and the URL box's edits that
 *  depend on whether they came from a paste. */
export function usePasteRequest({ active, activeRequest, env, open }: PasteRequestOptions): PasteRequest {
  /** A paste that the environment has names for, and the request it would become. Held rather
   *  than applied: the question is put to whoever pasted it, and both answers are cheap. */
  const [swap, setSwap] = useState<SwapOffer | null>(null);
  /** Set by a paste into the URL box that was not a whole request: the query that paste brings
   *  replaces the ticked Params rows at once rather than waiting for Send. */
  const pastedIntoUrl = useRef(false);

  /** The URL box changed. A query typed into it stays there until Send folds it into Params; one
   *  that arrived by paste takes the place of the ticked rows now. */
  function editUrl(url: string) {
    if (!activeRequest) return;
    const next = { ...activeRequest, url };
    if (pastedIntoUrl.current) {
      pastedIntoUrl.current = false;
      saveRequest(replaceQuery(next, () => crypto.randomUUID()));
      return;
    }
    saveRequest(next);
  }

  /** A paste into the URL box: a whole request is taken by `pasteInto`; anything else lands in the
   *  box as text, and the edit it makes goes through `replaceQuery`. The flag is dropped after the event either way,
   *  so a paste that changed nothing does not fold the next keystroke. */
  function pasteIntoUrl(text: string): boolean {
    if (pasteInto(text)) return true;
    pastedIntoUrl.current = true;
    setTimeout(() => {
      pastedIntoUrl.current = false;
    }, 0);
    return false;
  }

  /**
   * A paste in the URL box that turned out to be a cURL command.
   *
   * A tab nobody has put anything into is filled where it stands, and the request moves to Recent
   * with it — pressing New to have somewhere to paste into is not a decision to keep the result. A
   * tab with anything in it is left alone and the paste opens a tab of its own, which destroys
   * nothing and so needs no undo.
   *
   * Returns whether the paste was taken, which is what stops the box also receiving it.
   *
   * A command copied out of a browser has the host, the token and the api key written into it, and
   * those are the very things the environment beside it exists to hold. Where they match, the
   * offer to put the variables back is made — asked rather than done, because the values may be
   * the whole reason this particular command was pasted.
   */
  function pasteInto(text: string): boolean {
    const parsed = parsePaste(text, () => crypto.randomUUID());
    if (parsed === null) return false;
    const filled =
      activeRequest !== undefined && isBlank(activeRequest)
        ? pasteOverBlank(activeRequest, parsed)
        : pasteRequest(parsed);
    // The same id when the husk was filled in place; a different one when a duplicate was found in
    // Recent, and then it is that row's tab which comes forward.
    open(filled.id);
    if (env !== null) {
      const found = findSubstitutions(filled, env);
      if (found.length > 0) setSwap({ request: substitute(filled, env), found });
    }
    return true;
  }

  /**
   * The other way in: pasting a command with no request open at all.
   *
   * Without this the only paste target is a URL box, so a command could not be pasted without
   * pressing New first — and since a blank request is filled where it stands, that first paste
   * would never reach Recent. The empty screen says both ways in; this is the one it names first.
   *
   * Listened for on the document because nothing in an empty workspace holds the keyboard, so the
   * event has no element of ours to fire on. Which is also why the guards are needed: a paste aimed
   * at a text box is that box's, and a dialog or menu that is up holds the keyboard the same way it
   * holds every shortcut.
   */
  // `pasteInto` is remade every render; through an effect event the listener always runs the
  // current one without being re-subscribed for it.
  const pasteText = useEffectEvent((text: string) => pasteInto(text));
  useEffect(() => {
    if (!active || activeRequest !== undefined) return;
    function onPaste(event: ClipboardEvent) {
      if (modalDepth() !== 0) return;
      const target = event.target;
      if (
        target instanceof HTMLElement &&
        (target.isContentEditable || target.closest("input, textarea") !== null)
      ) {
        return;
      }
      if (pasteText(event.clipboardData?.getData("text") ?? "")) event.preventDefault();
    }
    document.addEventListener("paste", onPaste);
    return () => document.removeEventListener("paste", onPaste);
  }, [active, activeRequest]);

  return { editUrl, pasteIntoUrl, swap, setSwap };
}
