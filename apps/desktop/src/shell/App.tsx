import FirstRun from "./components/FirstRun";
import Workspace from "./Workspace";
import { useStartupProfile } from "./profiles";
/* Here rather than in `Workspace`, which is where they were until T108: the first-run screen is
   drawn *instead of* the workspace and wants the same tokens, the same ground and the same theme.
   Loading them from a component that may not mount would work only for as long as nobody makes
   `Workspace` a lazy import. */
import "./App.css";
import "./chrome.css";

/**
 * Which modules this window draws, decided before anything is drawn.
 *
 * A gate and not a branch inside the workspace, because `Workspace` settles its whole initial state
 * in `useState` initializers — which tabs to restore, which of them is active, which one to mount —
 * and every one of those reads the visible module list. Something that has to run before a
 * component's first render is that component's parent.
 *
 * `deciding` lasts one IPC round trip and happens on exactly one launch in the life of a machine —
 * a webview profile with no shell settings in it at all. It draws nothing rather than a spinner: a
 * spinner that appears and vanishes inside a frame is worse than a blank window that does not.
 */
function App() {
  const startup = useStartupProfile();

  if (startup.status === "deciding") return null;
  if (startup.status === "asking") return <FirstRun onChoose={startup.choose} />;
  return <Workspace enabled={startup.enabled} onEnabledChange={startup.setEnabled} />;
}

export default App;
