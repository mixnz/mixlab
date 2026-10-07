/**
 * What the Tunnel module calls everything.
 *
 * Pure data, importing nothing from `src/i18n/`: `dicts.ts` imports this file, so anything
 * importing back out of there would close a cycle.
 */
const tunnelEn = {
  tunnelTab: {
    newTabTitle: "Tunnel",
    settingsTitle: "Tunnel",
    title: "Tunnel",
    description: "Share an address on this machine on the internet through a Cloudflare quick tunnel.",
    address: "Local address",
    addressPlaceholder: "localhost:5173",
    start: "Start",
    stop: "Stop",
    copy: "Copy",
    copied: "Copied",
    open: "Open",
    anyoneWithTheLink: "Anyone with the link can open this while the tunnel runs.",
    forTesting: "Quick tunnels are for testing. Cloudflare promises no uptime, and Server-Sent Events do not work.",
    running: "Tunnels",
    stateConnecting: "Connecting…",
    stateOpen: "Open",
    stateFailed: "Stopped",
    empty: "No tunnel running.",
    columnAddress: "Address",
    columnState: "State",
    columnUrl: "Public URL",
    needCloudflared: "Tunnels run on cloudflared, and this machine has none yet.",
    download: "Download cloudflared",
    downloadSize: "cloudflared {{version}}, {{size}}",
    downloading: "Downloading…",
    using: "Using {{path}}",
    hintAllowedHosts: "The dev server refused the tunnel's address. Add it to server.allowedHosts in your Vite config.",
    hintTryIpv4: "Nothing answered on ::1. Try 127.0.0.1 instead of localhost.",
    pathLabel: "cloudflared to use",
    pathHint: "Leave empty to use one on PATH, or the one MixLab downloads.",
  },
  error: {
    tunnelTargetInvalid: "{{target}} is not an address.",
    tunnelTargetNotLocal: "{{target}} is not on this machine. A tunnel shares localhost, 127.0.0.1 or ::1.",
    tunnelNoBinary: "There is no cloudflared yet. Download it first.",
    tunnelNoDownload: "cloudflared has no download for this system.",
    tunnelCannotStart: "cloudflared could not start: {{message}}",
  },
};

export default tunnelEn;
