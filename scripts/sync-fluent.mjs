import { copyFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const uiDirectory = resolve(dirname(fileURLToPath(import.meta.url)), "../connector/ui");
const require = createRequire(resolve(uiDirectory, "package.json"));
const style = require.resolve("@lsypkg/fluent/style.css");
const license = resolve(dirname(style), "../LICENSE");

copyFileSync(style, resolve(uiDirectory, "fluent.css"));
copyFileSync(license, resolve(uiDirectory, "LICENSE.fluent"));
