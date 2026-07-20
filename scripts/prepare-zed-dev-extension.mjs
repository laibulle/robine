#!/usr/bin/env node

import { execFileSync } from "node:child_process";
import {
  cpSync,
  mkdtempSync,
  readFileSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import process from "node:process";

const repository = process.cwd();
const temporaryRoot = mkdtempSync(join(tmpdir(), "robine-zed-dev-"));
const grammarRepository = join(temporaryRoot, "tree-sitter-robine");
const extensionDirectory = join(temporaryRoot, "zed-robine");

cpSync(
  resolve(repository, "syntax/tree-sitter-robine"),
  grammarRepository,
  {
    recursive: true,
    filter: (source) => !source.includes("node_modules"),
  },
);
cpSync(resolve(repository, "editors/zed-robine"), extensionDirectory, {
  recursive: true,
  filter: (source) => !source.includes("/target"),
});

execFileSync("git", ["init", "-q"], { cwd: grammarRepository });
execFileSync("git", ["add", "."], { cwd: grammarRepository });
execFileSync(
  "git",
  [
    "-c",
    "user.name=Robine bootstrap",
    "-c",
    "user.email=bootstrap@robine.invalid",
    "-c",
    "commit.gpgsign=false",
    "commit",
    "-q",
    "-m",
    "Local grammar snapshot",
  ],
  { cwd: grammarRepository },
);
const revision = execFileSync("git", ["rev-parse", "HEAD"], {
  cwd: grammarRepository,
  encoding: "utf8",
}).trim();

const manifestPath = join(extensionDirectory, "extension.toml");
const manifest = readFileSync(manifestPath, "utf8")
  .replace(
    'repository = "https://github.com/laibulle/robine"\nrev = "main"\npath = "syntax/tree-sitter-robine"',
    `repository = "${pathToFileURL(grammarRepository).href}"\nrev = "${revision}"`,
  );
writeFileSync(manifestPath, manifest);

console.log(extensionDirectory);
