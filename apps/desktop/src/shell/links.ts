import type { Language } from "../i18n";

/**
 * Pages of MixLab's own that the app sends a user out to.
 *
 * Not in `version.ts`, which owns the release page: that is where a person goes to fetch a build,
 * and part of how the product is installed, while this is a document a user goes to read. They also
 * live on different hosts — the releases are on github.com, this is the project's Pages site — so a
 * reader who assumed one constant could be derived from the other would be wrong.
 */

/**
 * The privacy policy: a page of the handbook (`docs/guide/<lang>/privacy.md`), published by the
 * Pages workflow, in the language the app is set to. It replaced the standalone client's page, which said there was
 * no server of ours — true until sync.
 */
export function privacyPolicyUrl(lang: Language): string {
  return `https://mixnz.github.io/mixlab/${lang}/privacy/`;
}

/**
 * Running your own sync server, on Cloudflare Workers or in a Docker container: a page of the
 * handbook (`docs/guide/<lang>/self-hosting.md`), opened from the bottom of the Sync pane.
 */
export function selfHostingUrl(lang: Language): string {
  return `https://mixnz.github.io/mixlab/${lang}/self-hosting/`;
}
