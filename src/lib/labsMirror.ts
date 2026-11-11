import { LABS_FAILURE_NOTICES_KEY, LABS_OUTPUT_FIXES_KEY } from "@/lib/constants";
import { writeLocalStorage } from "@/lib/storage";
import type { LabsConfig } from "@/types";

/**
 * The subtitle overlay and History page cannot call `get_user_profile` (see
 * ipc_policy.rs), so the two Labs flags they read are mirrored into localStorage.
 * Call this whenever the profile's Labs block is loaded or saved, including after
 * a profile import, so the mirror never disagrees with the saved profile.
 */
export function mirrorLabsToStorage(labs: Pick<LabsConfig, "failureNotices" | "outputFixes">): void {
  writeLocalStorage(LABS_FAILURE_NOTICES_KEY, String(labs.failureNotices));
  writeLocalStorage(LABS_OUTPUT_FIXES_KEY, String(labs.outputFixes));
}
