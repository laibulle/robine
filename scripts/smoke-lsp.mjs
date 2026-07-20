#!/usr/bin/env node

import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { readFileSync } from "node:fs";
import { pathToFileURL } from "node:url";
import process from "node:process";

const binary = process.argv[2] ?? "target/debug/robine";
const child = spawn(binary, ["lsp", "--stdio"], {
  cwd: process.cwd(),
  stdio: ["pipe", "pipe", "inherit"],
});

let nextId = 1;
let buffer = Buffer.alloc(0);
const pending = new Map();
const notifications = [];

child.stdout.on("data", (chunk) => {
  buffer = Buffer.concat([buffer, chunk]);
  while (true) {
    const separator = buffer.indexOf("\r\n\r\n");
    if (separator === -1) {
      return;
    }
    const header = buffer.subarray(0, separator).toString("ascii");
    const match = header.match(/Content-Length:\s*(\d+)/i);
    assert(match, `missing Content-Length in ${header}`);
    const length = Number(match[1]);
    const bodyStart = separator + 4;
    if (buffer.length < bodyStart + length) {
      return;
    }
    const body = buffer.subarray(bodyStart, bodyStart + length).toString("utf8");
    buffer = buffer.subarray(bodyStart + length);
    const message = JSON.parse(body);
    if (Object.hasOwn(message, "id") && pending.has(message.id)) {
      const { resolve, reject } = pending.get(message.id);
      pending.delete(message.id);
      if (message.error) {
        reject(new Error(JSON.stringify(message.error)));
      } else {
        resolve(message.result);
      }
    } else if (message.method) {
      notifications.push(message);
    }
  }
});

function send(message) {
  const body = JSON.stringify({ jsonrpc: "2.0", ...message });
  child.stdin.write(`Content-Length: ${Buffer.byteLength(body)}\r\n\r\n${body}`);
}

function request(method, params) {
  const id = nextId++;
  send({ id, method, params });
  return new Promise((resolve, reject) => pending.set(id, { resolve, reject }));
}

function notify(method, params = {}) {
  send({ method, params });
}

async function waitForNotification(method) {
  const deadline = Date.now() + 5_000;
  while (Date.now() < deadline) {
    const index = notifications.findIndex((item) => item.method === method);
    if (index !== -1) {
      return notifications.splice(index, 1)[0].params;
    }
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
  throw new Error(`timeout waiting for ${method}`);
}

const examplePath = `${process.cwd()}/examples/hello/src/main.ro`;
const validSource = readFileSync(examplePath, "utf8");
const invalidSource = validSource.replace(" ! { Console.Write }", "");
const uri = pathToFileURL(examplePath).href;

function positionOf(source, needle, occurrence = 0) {
  let offset = -1;
  for (let index = 0; index <= occurrence; index += 1) {
    offset = source.indexOf(needle, offset + 1);
    assert.notEqual(offset, -1, `missing ${needle} occurrence ${occurrence}`);
  }
  const before = source.slice(0, offset);
  const lines = before.split("\n");
  return {
    line: lines.length - 1,
    character: [...lines.at(-1)].join("").length,
  };
}

try {
  const initialize = await request("initialize", {
    processId: process.pid,
    rootUri: pathToFileURL(process.cwd()).href,
    capabilities: {
      general: {
        positionEncodings: ["utf-16"],
      },
    },
  });
  assert.equal(initialize.capabilities.positionEncoding, "utf-16");
  notify("initialized");

  notify("textDocument/didOpen", {
    textDocument: {
      uri,
      languageId: "robine",
      version: 1,
      text: invalidSource,
    },
  });
  const invalidDiagnostics = await waitForNotification(
    "textDocument/publishDiagnostics",
  );
  assert.equal(invalidDiagnostics.version, 1);
  assert(
    invalidDiagnostics.diagnostics.some(
      (diagnostic) => diagnostic.code === "RBN4001",
    ),
    "missing effect diagnostic was not published",
  );

  notify("textDocument/didChange", {
    textDocument: { uri, version: 2 },
    contentChanges: [{ text: validSource }],
  });
  const validDiagnostics = await waitForNotification(
    "textDocument/publishDiagnostics",
  );
  assert.equal(validDiagnostics.version, 2);
  assert.deepEqual(validDiagnostics.diagnostics, []);

  const hover = await request("textDocument/hover", {
    textDocument: { uri },
    position: positionOf(validSource, "console", 1),
  });
  assert.match(hover.contents.value, /Console/);

  const symbols = await request("textDocument/documentSymbol", {
    textDocument: { uri },
  });
  assert(symbols.some((symbol) => symbol.name === "main"));
  assert(symbols.some((symbol) => symbol.name === "fibonacci"));

  const definition = await request("textDocument/definition", {
    textDocument: { uri },
    position: positionOf(validSource, "fibonacci", 3),
  });
  assert.equal(definition.uri, uri);
  assert.deepEqual(
    definition.range.start,
    positionOf(validSource, "fibonacci", 0),
  );

  const completion = await request("textDocument/completion", {
    textDocument: { uri },
    position: positionOf(validSource, "console", 1),
  });
  assert(completion.some((item) => item.label === "main"));
  assert(completion.some((item) => item.label === "fibonacci"));
  assert(completion.some((item) => item.label === "console"));
  assert(completion.some((item) => item.label === "if"));

  const formatting = await request("textDocument/formatting", {
    textDocument: { uri },
    options: { tabSize: 4, insertSpaces: true },
  });
  assert.equal(formatting[0].newText, validSource);

  await request("shutdown", null);
  notify("exit");
  console.log(
    "OK — LSP diagnostics, snapshots, hover, definition, completion, symbols and formatting",
  );
} finally {
  await new Promise((resolve) => child.stdin.end(resolve));
  const gracefulExit =
    child.exitCode ??
    (await Promise.race([
      new Promise((resolve) => child.once("exit", resolve)),
      new Promise((resolve) => setTimeout(() => resolve("timeout"), 2_000)),
    ]));
  if (gracefulExit === "timeout") {
    child.kill("SIGTERM");
    throw new Error("language server did not exit after shutdown/exit");
  }
  const exitCode = gracefulExit;
  assert.equal(exitCode, 0);
}
