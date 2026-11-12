// The built-in plugins, in plugin bar order. The only core file that imports
// a plugin's own folder.

import { dbPlugin } from "./db/index.tsx";
import { dockerPlugin } from "./docker/index.tsx";
import { gitPlugin } from "./git/index.tsx";
import type { WestermPlugin } from "./types.ts";

export const BUILTIN_PLUGINS: readonly WestermPlugin[] = [gitPlugin, dockerPlugin, dbPlugin];
