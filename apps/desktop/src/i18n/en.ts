const en = {
  common: {
    host: "Host",
    port: "Port",
    user: "User",
    password: "Password",
    database: "Database",
    connect: "Connect",
    disconnect: "Disconnect",
    save: "Save",
    cancel: "Cancel",
    confirm: "Confirm",
    delete: "Delete",
    duplicate: "Duplicate",
    browse: "Browse...",
    close: "Close",
    scrollTabsLeft: "Scroll tabs left",
    scrollTabsRight: "Scroll tabs right",
    loading: "Loading...",
    // Why anything that would write is greyed out, wherever in the workspace it is. One sentence
    // rather than one per panel: it is the same fact, and it names where to undo it.
    readOnlyConnection: "This connection is marked read-only. Change it from the connection's right-click menu.",
    // The same fact in the space a badge has: beside the name in the sidebar, and in the tab of a
    // connection that is already open.
    readOnly: "Read-only",
  },
  app: {
    settings: "Settings",
    settingsDownloading: "Settings (downloading changes)",
    settingsUploading: "Settings (uploading changes)",
    /** The name of the app's own tab bar, for anyone reading the screen rather than looking at it. */
    tabs: "Open tabs",
    closeTab: "Close tab",
    /** The same button on the last tab there is, where closing it opens a fresh one in its place —
     *  so what it does is reload the module, and that is what it says. */
    reloadTab: "Reload module",
    newConnectionTab: "New connection tab",
    newConnectionTitle: "New Connection",
    /* What each module is called in the `[+]` menu — see `shell/registry.ts`, which is the list
       the menu is built from. */
    moduleDatabase: "Database",
    moduleRest: "REST",
    moduleTerminal: "Terminal",
    moduleTools: "Tools",
    moduleMixEngine: "MixEngine",
  },
  /* Which modules this window draws — T108. The first-run screen and the Settings pane share these
     words on purpose: a person meets the three presets once at the start and finds the same three
     names when they go looking for them again. */
  profiles: {
    title: "Modules",
    question: "What will you use MixLab for?",
    changeLater: "You can change this in Settings at any time.",
    presetMixengine: "MixEngine",
    presetMixengineAbout: "Sites, runtimes and services. The window MixEngine ships with.",
    presetEverything: "Everything",
    presetEverythingAbout: "MixEngine, and the database, REST and terminal tools.",
    presetDatabaseTools: "Database tools",
    presetDatabaseToolsAbout: "The database client, REST client, terminal and tools.",
    presets: "Presets",
    shown: "Modules shown",
    lastOne: "At least one module has to stay on.",
    confirmTitle: "Close these tabs?",
    confirmMessage:
      "Tabs open in {{modules}} will be closed. Nothing you have saved is deleted, and turning the module back on finds it where it was.",
    confirmAction: "Turn off and close",
    turnedOn:
      "{{module}} was turned on so this tab could open. Turn it off again in Settings → Modules.",
    turnedOnDismiss: "Dismiss this notice",
  },
  pagination: {
    previousPage: "Previous page",
    nextPage: "Next page",
    status: "Page {{page}} of {{pageCount}} \u00b7 {{total}} rows",
    perPage: "{{n}} / page",
  },
  select: {
    placeholder: "Select...",
    noOptions: "No options",
    noMatches: "No matches",
    searchPlaceholder: "Search...",
    useTyped: 'Use "{{value}}"',
  },
  input: {
    clear: "Clear",
  },
  errorBanner: {
    dismiss: "Dismiss error",
  },
  noticeBanner: {
    dismiss: "Dismiss notice",
  },
  cellDialog: {
    title: "{{column}}, row {{n}}",
    copy: "Copy",
  },
  settings: {
    title: "Settings",
    appearance: "Appearance",
    general: "General",
    theme: "Theme",
    themeLight: "Light",
    themeDark: "Dark",
    themeSystem: "System",
    accent: "Accent colour",
    accentMint: "Mint",
    accentBlue: "Blue",
    accentIndigo: "Indigo",
    accentViolet: "Violet",
    accentMagenta: "Magenta",
    accentOrange: "Orange",
    accentAmber: "Amber",
    accentGreen: "Green",
    accentTeal: "Teal",
    accentCyan: "Cyan",
    accentSlate: "Slate",
    language: "Language",
    languageEnglish: "English",
    languageVietnamese: "Ti\u1ebfng Vi\u1ec7t",
    privacyPolicy: "Privacy policy",
    privacyHint:
      "MixLab collects nothing about you. What it keeps stays on this machine. If you turn sync on, the server holds only what it cannot read.",
    logHint: "A file on this machine records crashes and errors, in case something needs a closer look.",
    openLogFolder: "Open log folder",
  },
  // The Ctrl/Cmd chords the app answers, as Settings lists them. A module's own chords are named in
  // that module's dictionary, beside the rest of its words — see `src/i18n/dicts.ts`, which will
  // not let two dictionaries claim the same group.
  shortcuts: {
    title: "Shortcuts",
    scope: {
      app: "App",
    },
    newTab: "New tab",
    // One row per module, filled from the module's own name — see `shell/shortcuts.ts`.
    newModuleTab: "New {{module}} tab",
    /** The same row for a module that holds one tab and no more: the key opens it where there is
     *  none and goes to it where there is one, which is one thing and is said as one. */
    goToModule: "Go to {{module}}",
    closeTab: "Close tab",
    nextTab: "Next tab",
    prevTab: "Previous tab",
    reload: "Reload the pane on screen",
  },
  // The tray (T168, T192): the frame's header, and the menu's items — sent to
  // `src-tauri/src/tray.rs` rather than kept in Rust. The slogan is a brand line and stays in
  // English in every language.
  tray: {
    openPanel: "Open control panel",
    openMain: "Open MixLab",
    quit: "Quit MixLab",
    slogan: "For Developers. By Developers.",
  },
  // MixLab at login (ADR 0042, ADR 0058): the shell's General pane, whatever modules are visible.
  loginItem: {
    title: "At login",
    toggle: "Open MixLab in the tray when I log in",
    about: "Only the tray icon appears. MixEngine has its own switch in its Settings pane.",
    noTray: "Your desktop shows no tray icons, so MixLab will open its window at login instead.",
    unsupported: "A development build does not start at login.",
  },
  // MixLab's own updater, in Settings → Updates (T187). It checks, downloads and installs whether
  // or not MixEngine is running, and never installs without a click.
  update: {
    title: "Updates",
    runningNow: "You're running {{version}}",
    checkedAt: "Last checked at {{time}}.",
    neverChecked: "Not checked yet.",
    checkNow: "Check now",
    checking: "Checking",
    checkFailed: "Couldn't check for updates. {{message}}",
    automatic: "Check for updates automatically",
    automaticHint: "MixLab looks once when it starts and once a day. It never installs anything until you click Install.",
    upToDate: "You're on the latest version.",
    offered: "MixLab {{version}} is available",
    size: "{{size}} MB to download.",
    notesLink: "Read the release notes",
    daemonRestarts: "If MixEngine is running, it stops for the update and starts again with the same services.",
    download: "Download",
    cancel: "Cancel",
    ready: "MixLab {{version}} is ready to install",
    installerDownloaded: "The installer for {{version}} is downloaded",
    installRestart: "Install and restart",
    readyRestartsDaemon: "MixEngine will restart.",
    readyRestartsServices: "MixEngine and {{count}} services will restart.",
    received: "{{received}} of {{total}} MB",
    tryAgain: "Try again",
    close: "Close",
    openInstaller: "Open installer",
    later: "Later",
    skip: "Skip this version",
    skipped: "You skipped {{version}}. MixLab will tell you about the next one.",
    downloading: "Downloading {{percent}}%",
    installing: "Installing",
    installerOpen: "The installer is open. Finish it there, then come back here.",
    installerNotOpened: "The installer is downloaded. Run this command to install it:",
    installerCommand: "Or install it from a terminal:",
    installerFinish: "Finish",
    installerReady: "{{version}} is installed. Click Finish to restart MixLab on it.",
    installerReopen: "Open again",
    installerBack: "Back",
    noBuild: "{{version}} has no build for this machine yet.",
    development: "This is a development build, so it doesn't update itself.",
    elsewhere: "Something other than MixLab's installer put this copy here, so MixLab can't update it. Get the new version from the download page.",
    openPage: "Open the download page",
    available: "MixLab {{version}} is available.",
  },
  // MixLab ▸ Remove MixLab from this Mac… (T182a): the dialog that removes MixLab from a Mac the
  // .pkg installed it on, then quits.
  remove: {
    title: "Remove MixLab from this Mac",
    menuItem: "Remove MixLab from this Mac…",
    checking: "Checking what is on this Mac…",
    intro: "This undoes everything MixEngine changed on this Mac, then removes MixLab itself and quits. You'll be asked for your password once.",
    blocked: "{{what}}: {{by}}",
    checkAgain: "Check again",
    deleteData: "Also delete MixLab's data in {{path}}: your databases, certificates and project records",
    deleteRelocated: "Also delete the folders you moved out of it",
    remove: "Remove MixLab",
    removing: "Removing",
    waiting: "Waiting for your password, then removing.",
    declined: "You didn't allow the prompt, so nothing was removed. MixLab is still installed.",
    failed: "Some things could not be removed. MixLab stays installed, so you can run this again once they're fixed.",
    daemonStayed: "Everything was removed, but MixEngine is still running. Quit it and run this again to finish.",
    left: "Still on this Mac:",
  },
  // What a failed backend command says. The keys here are the `code` an `AppError` carries \u2014 see
  // src-tauri/src/error.rs \u2014 and `{{message}}` is where a driver's own words go, untranslated
  // because they are the server talking and the part worth searching for.
  error: {
    // MixEngine — the local daemon this app manages. `message` is the daemon's own words and is
    // never translated: it is what a search engine and MixEngine's own manual both index.
    // Restarting the window — T106, `src-tauri/src/relaunch.rs`. The first is a machine whose
    // operating system will not name this process's own executable; the second is one that would
    // not start it.
    relaunchNoExecutable: "MixLab could not work out which program to start again.",
    relaunchFailed: "MixLab could not start itself again: {{message}}",
    mixengineNoHome: "Could not work out where MixEngine keeps its files.",
    mixengineUnreachable: "No MixEngine daemon answered at {{endpoint}}.",
    mixenginePipeOwner:
      "The MixEngine pipe at {{endpoint}} is held by {{owner}}, not by this account.",
    mixengineRefused: "MixEngine refused: {{message}}",
    mixengineStartFailed: "Could not start MixEngine: {{message}}",
    uninstallNoDaemon: "MixEngine could not be started, so nothing was removed. From a terminal: mix uninstall --package",
    uninstallFailed: "The removal stopped: {{message}}",
    uninstallUnavailable: "This copy of MixLab was not installed from the macOS package, so it can't remove itself.",
    updateDaemonWouldNotStop: "MixEngine did not stop within 30 seconds, so nothing was replaced. Try again in a moment.",
    updateNoBuild: "This release has no build for this machine.",
    updateLocked: "Another update is running (process {{pid}}). Try again when it finishes.",
    updateUnwritable: "MixLab can't write to the folder it's installed in, so it can't update itself here.",
    updateFailed: "The update didn't install, and nothing was changed. {{message}}",
    updateNotDownloaded: "The update isn't on this computer any more. Download it again.",
    updateOldInUse: "{{path}} is still in use. Close the program running from it, then try again.",
    updateCancelled: "The download was stopped.",
    updateDownloading: "The update is already downloading.",
    updateDaemonNotBack: "MixLab was updated, but MixEngine didn't start again. Start it from the MixEngine tab. {{cause}}",
    mixengineStorageFailed: "Could not read where MixEngine keeps its files: {{message}}",
    mixengineProtocol: "MixEngine answered something this version does not understand: {{message}}",
    // SSH
    sshTimeout:
      "The SSH connection to {{host}}:{{port}} timed out after {{seconds}}s. Check the host, the port and the firewall.",
    sshConnectFailed: "Cannot reach the SSH server: {{message}}",
    sshAuthFailed: "SSH authentication failed: {{message}}",
    sshShellFailed: "Could not open a shell on the SSH server: {{message}}",
    trayUnavailable: "The tray icon could not be created: {{message}}",
    loginItemUnsupported: "A development build does not start at login.",
    loginItemFailed: "The login entry could not be changed: {{message}}",
    sshAuthRejected:
      "The SSH server rejected the login (partial success: {{partialSuccess}}). It accepts: {{methods}}.",
    sshHostKeyChanged:
      "The SSH server at {{endpoint}} is offering a different key than the one MixLab saw before ({{fingerprint}} now, {{known}} before). The server may have been rebuilt, or something may be standing between you and it. If you expected the change, remove its entry from {{file}} and connect again.",
    cannotReadPrivateKey: "Cannot read the private key file: {{message}}",
    invalidPrivateKey: "That is not a private key MixLab can read: {{message}}",
    cannotBindTunnelPort: "Cannot open a local port for the tunnel: {{message}}",
    tunnelAcceptFailed:
      "The tunnel's local port has stopped taking connections: {{message}}. MixLab keeps trying. If it does not come back, close the tab and connect again.",
    cannotSaveKnownHost: "Cannot remember the server's key: {{message}}",
    sshUnavailable:
      "The SSH tunnel is not open. MixLab is trying to open it again.",
    // Saved passwords
    credentialStoreUnreachable: "Cannot reach the system credential store: {{message}}",
    cannotSavePassword: "Cannot save the password: {{message}}",
    cannotReadPassword: "Cannot read the saved password back: {{message}}",
    syncKeyDerivation: "Could not derive the sync keys: {{message}}",
    syncCannotWrapKey: "Could not protect the sync key.",
    syncCannotUnwrapKey: "Wrong password, or this account's key has been damaged.",
    syncRecoveryKeyUnreadable:
      "That is not a recovery key. Check it against what you wrote down.",
    syncCannotSealRecord: "Could not encrypt this item for sync.",
    syncCannotOpenRecord:
      "An item from the server could not be read. It may have been tampered with.",
    syncSignedOut: "You were signed out of sync. Sign in again.",
    syncAccessTokenRejected:
      "This sync server needs an access token, and the one set was not accepted.",
    syncAccountFrozen:
      "Your account is being copied to another server and cannot change right now.",
    syncTooManyRequests: "The sync server is busy. Try again in {{seconds}} seconds.",
    syncQuotaExceeded: "Your sync account is full.",
    syncRequestTooLarge: "Too much to sync in one go.",
    syncRecordTooLarge: "This item is larger than the sync server accepts.",
    syncServerUnreachable: "Could not reach the sync server: {{message}}",
    syncServerRefused: "The sync server refused the request ({{code}}).",
    syncServerAnswerUnreadable: "The sync server's answer could not be read.",
    syncCannotEncodeRequest: "Could not prepare the sync request.",
    syncStoreFailed: "Could not save the sync state: {{message}}",
    syncConflictUnresolved: "Another machine kept changing this item. Sync will try again.",
    syncWrongPassword: "That address and password do not match an account.",
    syncEmailNotVerified: "Confirm your address with the code in the letter before signing in.",
    syncEmailTaken: "That address already has an account. Sign in instead.",
    syncInvalidEmail: "That is not an address a letter could reach.",
    syncInvalidDeviceName: "This machine needs a name.",
    syncLetterNotSent: "The confirmation letter could not be sent, so no account was made. Try again later.",
    syncWrongCode: "That code is wrong, already used, or expired.",
    syncSavedUnreadable: "The saved sync sign-in could not be read. Sign in again.",
    syncNotSignedIn: "Not signed in to sync.",
    syncNothingToVerify: "There is no sign-up waiting for a code. Sign up again.",
    syncPageStale: "Sync changed while this was being applied. It will run again.",
    syncArgonUnsupported: "This account was made by a client MixLab cannot sign in for.",
    syncResetExpired: "That reset took too long. Ask for a new code.",
    syncRecoveryKeyWrong: "That recovery key does not open this account.",
    syncNothingToReset: "There is no password reset under way. Start again.",
    syncMoveIncomplete:
      "{{missing}} records did not arrive on the new server. The old account stays frozen: try again, or stop the move.",
    syncSignInAgainToMove: "Sign out and sign in again on this machine before moving the account.",
    syncNothingToMove: "There is no move under way. Start again.",
    cannotRemovePassword: "Cannot remove the saved password: {{message}}",

    // The two both layers raise: a directory the app makes for itself, and work handed to a
    // background thread. The database module emits these as well, and reads them from here.
    cannotCreateDirectory: "Cannot create {{path}}: {{message}}",
    backgroundTaskFailed: "The task did not finish: {{message}}",
    // The one error in here the webview raises rather than the backend. Said out loud because the
    // alternative is a copy that did nothing and a paste, somewhere else, of what was there before.
    clipboard: "Nothing was copied. The clipboard refused: {{message}}",
    /** An error shape MixLab doesn't recognise \u2014 shown as-is rather than swallowed. */
    unknown: "{{message}}",
    /** The Error Boundary around one tab \u2014 the rest of the app (other tabs, the update check) is
     *  still alive. */
    crashedTab: "This tab hit a bug and could not go on. The rest of MixLab is unaffected.",
    /** The outermost Error Boundary \u2014 the whole App failed to render. No "Try again": there is
     *  nothing left to try it into, only a restart. */
    crashedApp: "MixLab hit a bug it could not recover from.",
    tryAgain: "Try again",
    restartApp: "Restart MixLab",
  },
  sync: {
    preferences: "Preferences",
    recoveryTitle: "Your recovery key",
    recoveryHint:
      "Write it down and keep it somewhere safe. It is shown only once. If you forget your password, this key and your email get your data back.",
    recoveryCopy: "Copy",
    recoveryWritten: "I have written it down",
    recoveryCheck: "Type groups {{first}} and {{second}} back",
    recoveryGroup: "Group {{n}}",
    recoveryMismatch: "Those groups do not match the key.",
    recoveryShowAgain: "Show the key again",
    recoveryLost:
      "The recovery key was shown once and cannot be shown again. If you did not write it down, start over.",
    startOver: "Start over",
    codeTitle: "Check your email",
    codeHint: "A code was sent to {{email}}.",
    code: "Code",
    deviceName: "This machine's name",
    confirming: "Confirming",
    continue: "Continue",
    signedInAs: "Signed in as {{email}}",
    signOut: "Sign out",
    signingOut: "Signing out",
    signOutHint: "Signing out stops sync on this machine. What is on it stays on it.",
    syncNow: "Sync now",
    syncing: "Syncing",
    syncedJustNow: "Synced a moment ago",
    syncedAgo: "Synced {{when}}",
    syncFailed: "Last sync failed: {{message}}",
    closing: "This server closes on {{date}}. Copy your account to another server before then.",
    replaced: "Edits you made in {{collection}} were replaced by newer ones from another machine ({{count}}).",
    collections: "What syncs",
    collectionsHint: "Every row starts off. Turn on only what you want on your other machines.",
    devices: "Machines",
    thisDevice: "This machine",
    lastSeen: "Last seen {{when}}",
    revoke: "Remove",
    revokeTitle: "Remove {{name}}?",
    revokeMessage: "It is signed out at once and stops syncing. What it already has stays on it.",
    title: "Sync",
    intro:
      "Sync keeps a copy of what you choose on a server that cannot read it: everything is encrypted on this machine first. Nothing leaves until you turn a row on.",
    selfHostingHint:
      "You can also sync through a server you run yourself, on Cloudflare Workers or in Docker.",
    selfHostingGuide: "Read the guide",
    server: "Server",
    serverDefault: "{{url}} (default)",
    addServer: "Add a server",
    addServerAction: "Add",
    removeServer: "Remove",
    serverInvalid: "That is not a server address.",
    serverInsecure: "Use https://. Plain http:// works only for a server on this machine.",
    access: "Access token",
    accessHint: "Only for a server its owner has closed to their own people.",
    email: "Email",
    passwordAgain: "Password, again",
    passwordsDiffer: "The two passwords are different.",
    passwordHint:
      "Only your recovery key can reset this password and keep your data. Choose one you will remember.",
    signIn: "Sign in",
    signUp: "Create an account",
    signingIn: "Signing in",
    signingUp: "Creating the account",
    changePassword: "Change password",
    currentPassword: "Current password",
    newPassword: "New password",
    changePasswordHint: "Your other machines are signed out and will ask for the new one. Your data stays as it is.",
    passwordChanged: "Password changed. Your other machines were signed out and will ask for the new one.",
    saving: "Saving",
    forgot: "Forgot your password?",
    forgotTitle: "Reset your password",
    forgotHint:
      "A code will be sent to your address at {{server}}. With your recovery key your data stays; without it, it is deleted.",
    sendCode: "Send code",
    sendingCode: "Sending",
    haveKey: "I have my recovery key",
    noKey: "I don't have it",
    recoveryKey: "Recovery key",
    resetDeletesEverything:
      "Without the recovery key nothing can read your data any more, so resetting deletes everything this account holds on the server. What is on your machines stays there.",
    resetKeep: "Reset password",
    resetStartOver: "Delete everything and start over",
    resetting: "Resetting",
    finishTitle: "Last step",
    deleteAccount: "Delete account",
    deleteAccountWarning:
      "This deletes your account on {{server}} and everything it holds there, and signs every machine out. What is on your machines stays there.",
    deleteAccountAction: "Delete the account",
    deleting: "Deleting",
    frozen:
      "This account has been held still for a copy to another server since {{date}}. Nothing can sync until the copy ends.",
    thaw: "End the copy",
    moveTitle: "Move to another server",
    moveHint:
      "Your account is copied to the new server unchanged, then you choose what happens to this one. While the copy runs, nothing can sync.",
    moveTo: "New server",
    moving: "Working",
    moveCodeHint: "{{server}} sent a code to your address. Confirming it starts the copy.",
    moveCopy: "Confirm and copy",
    copying: "Copying",
    moveCopied: "{{count}} records are on {{server}}, all of them checked. What should happen to the old account?",
    moveEnd: "The old account",
    moveDeleteOld: "Delete it",
    moveKeepOld: "Keep it",
    moveDeleteOldHint:
      "Any machine still pointed at the old server is signed out today, so you notice it. This is the safer choice.",
    moveKeepOldHint:
      "The old account opens again. A machine still pointed at it goes on writing there, where no machine of yours reads.",
    moveFinish: "Finish the move",
    moveAbandon: "Stop the move",
    closingSoon: "{{server}} closes in {{days}} days, on {{date}}. Move your account before then.",
    closingNow: "{{server}} was to close on {{date}}. Move your account now, while it still answers.",
    openSync: "Open Sync",
    serverClosing: "{{server}} has announced it closes on {{date}}. An account there will have to move before then.",
    secretNeedsOwner: "Turn on {{collection}} first.",
    secretConfirmTitle: "Sync {{collection}}?",
    secretConfirmMessage:
      "MixLab encrypts them on this machine before they leave. The server can't read them, but every machine signed in to your account can.",
    secretConfirmAction: "Sync them",
  },
};

/** The half of the dictionary no module owns. `src/i18n/dicts.ts` merges it with each module's. */
export type SharedDict = typeof en;

export default en;
