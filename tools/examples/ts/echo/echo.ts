// clawbox echo tool — TypeScript
// Reads JSON from stdin, echoes it back with metadata.
// Build: TS → JS (esbuild) → WASM (javy)

declare const Javy: {
  IO: {
    readSync(fd: number): Uint8Array;
    writeSync(fd: number, data: Uint8Array): void;
  };
};

interface EchoParams {
  [key: string]: unknown;
}

interface EchoResponse {
  tool: string;
  version: string;
  echo: EchoParams;
  message: string;
}

const input: Uint8Array = Javy.IO.readSync(0);
const decoder = new TextDecoder();
const params: EchoParams = JSON.parse(decoder.decode(input));

const response: EchoResponse = {
  tool: "echo-ts",
  version: "0.1.0",
  echo: params,
  message: "Hello from clawbox WASM sandbox! (TypeScript)",
};

const encoder = new TextEncoder();
const encoded: Uint8Array = encoder.encode(JSON.stringify(response));
Javy.IO.writeSync(1, encoded);
