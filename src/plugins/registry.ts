// The built-in plugins, in plugin bar order. The only core file that imports
// a plugin's own folder.

import { dbPlugin } from "./db/index.tsx";
import { dockerPlugin } from "./docker/index.tsx";
import type { WestermPlugin } from "./types.ts";

export const BUILTIN_PLUGINS: readonly WestermPlugin[] = [dockerPlugin, dbPlugin];
