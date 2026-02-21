// clawbox echo tool — JavaScript
// Reads JSON from stdin, echoes it back with metadata.
// Requires Javy v8+ (two-arg readSync API).

function readStdin() {
  const chunks = [];
  const buf = new Uint8Array(4096);
  while (true) {
    const n = Javy.IO.readSync(0, buf);
    if (n === 0) break;
    chunks.push(buf.slice(0, n));
  }
  const total = chunks.reduce((s, c) => s + c.length, 0);
  const result = new Uint8Array(total);
  let offset = 0;
  for (const chunk of chunks) {
    result.set(chunk, offset);
    offset += chunk.length;
  }
  return new TextDecoder().decode(result);
}

const params = JSON.parse(readStdin());

const response = {
  tool: "echo-js",
  version: "0.1.0",
  echo: params,
  message: "Hello from clawbox WASM sandbox! (JavaScript)"
};

const encoder = new TextEncoder();
const encoded = encoder.encode(JSON.stringify(response));
Javy.IO.writeSync(1, encoded);
