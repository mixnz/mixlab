import { useEffect, useRef, useState } from "react";
import { findEnvironment, type Environment } from "../environments";
import { setLastEnvId, workspaceLoaded, type Workspace } from "../workspace";

export interface EnvironmentChoice {
  /** Null when nothing is chosen, and also when what was chosen has since been deleted — which is
   *  the whole of what deleting an environment has to clean up. */
  env: Environment | null;
  chooseEnv: (id: string | null) => void;
}

/** The environment this tab sends with: the last one chosen in any REST tab, taken once, and this
 *  tab's own choice from then on. */
export function useEnvironmentChoice(workspace: Workspace, environments: Environment[]): EnvironmentChoice {
  const [envId, setEnvId] = useState<string | null>(null);
  /** Whether `lastEnvId` has been taken. Once, and once only — see the note on the field. */
  const envSeeded = useRef(false);

  useEffect(() => {
    if (envSeeded.current || !workspaceLoaded()) return;
    envSeeded.current = true;
    setEnvId(workspace.lastEnvId);
  }, [workspace]);

  const env = findEnvironment(environments, envId);

  function chooseEnv(id: string | null) {
    setEnvId(id);
    // Written for the next REST tab to open with; this one keeps its own choice from here.
    setLastEnvId(id);
  }

  return { env, chooseEnv };
}
