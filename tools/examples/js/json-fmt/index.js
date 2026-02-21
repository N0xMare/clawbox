function readStdin(){const c=[];const b=new Uint8Array(4096);while(true){const n=Javy.IO.readSync(0,b);if(n===0)break;c.push(b.slice(0,n))}const t=c.reduce((s,x)=>s+x.length,0);const r=new Uint8Array(t);let o=0;for(const x of c){r.set(x,o);o+=x.length}return new TextDecoder().decode(r)}
const params = JSON.parse(readStdin());
const action = params.action || "format";
const data = params.data;
const indent = params.indent ?? 2;
let result;
try {
  switch (action) {
    case "validate": {
      try { JSON.parse(typeof data === "string" ? data : JSON.stringify(data)); result = { valid: true }; }
      catch (e) { result = { valid: false, error: e.message }; }
      break;
    }
    case "format": {
      const parsed = typeof data === "string" ? JSON.parse(data) : data;
      result = { formatted: JSON.stringify(parsed, null, indent) };
      break;
    }
    case "minify": {
      const parsed = typeof data === "string" ? JSON.parse(data) : data;
      result = { minified: JSON.stringify(parsed) };
      break;
    }
    case "keys": {
      const parsed = typeof data === "string" ? JSON.parse(data) : data;
      result = (typeof parsed === "object" && parsed !== null) ? { keys: Object.keys(parsed) } : { error: "Input is not an object" };
      break;
    }
    case "extract": {
      const parsed = typeof data === "string" ? JSON.parse(data) : data;
      let current = parsed;
      for (const key of (params.path || "").split(".").filter(Boolean)) {
        current = (current && typeof current === "object") ? current[key] : undefined;
      }
      result = { value: current };
      break;
    }
    default: result = { error: "Unknown action: " + action };
  }
} catch (e) { result = { error: e.message }; }
Javy.IO.writeSync(1, new TextEncoder().encode(JSON.stringify(result)));
