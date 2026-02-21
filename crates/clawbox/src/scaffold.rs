use anyhow::{Result, bail};
use std::fs;
use std::path::Path;

pub fn scaffold(name: &str, lang: &str, dir: Option<&str>) -> Result<()> {
    let base = match dir {
        Some(d) => Path::new(d).join(name),
        None => Path::new(name).into(),
    };

    if base.exists() {
        bail!("Directory already exists: {}", base.display());
    }

    match lang {
        "rust" => scaffold_rust(&base, name)?,
        "js" => scaffold_js(&base, name)?,
        "ts" => scaffold_ts(&base, name)?,
        _ => bail!("Unsupported language: {lang}. Use rust, js, or ts."),
    }

    eprintln!("✓ Scaffolded {lang} tool '{name}' at {}", base.display());
    Ok(())
}

fn write_manifest(dir: &Path, name: &str) -> Result<()> {
    let manifest = format!(
        r#"{{
  "tool": {{
    "name": "{name}",
    "description": "A clawbox WASM tool",
    "version": "0.1.0"
  }},
  "input_schema": {{
    "type": "object",
    "properties": {{}},
    "additionalProperties": true
  }}
}}"#
    );
    fs::write(dir.join("manifest.json"), manifest)?;
    Ok(())
}

fn scaffold_rust(dir: &Path, name: &str) -> Result<()> {
    fs::create_dir_all(dir.join("src"))?;

    let cargo_toml = format!(
        r#"[package]
name = "{name}"
version = "0.1.0"
edition = "2021"

[dependencies]
serde = {{ version = "1", features = ["derive"] }}
serde_json = "1"
"#
    );
    fs::write(dir.join("Cargo.toml"), cargo_toml)?;

    let main_rs = r#"use serde_json::{json, Value};
use std::io::{self, Read};

fn main() {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input).unwrap();
    let params: Value = serde_json::from_str(&input).unwrap_or(json!({}));

    // TODO: Implement your tool logic here
    let output = json!({
        "tool": env!("CARGO_PKG_NAME"),
        "result": params
    });

    print!("{}", output);
}
"#;
    fs::write(dir.join("src/main.rs"), main_rs)?;
    write_manifest(dir, name)?;
    Ok(())
}

fn scaffold_js(dir: &Path, name: &str) -> Result<()> {
    fs::create_dir_all(dir)?;

    let tool_js = r#"// Read input from stdin
const input = [];
const buf = new Uint8Array(1024);
let n;
while ((n = Javy.IO.readSync(0, buf)) > 0) {
    input.push(...buf.subarray(0, n));
}
const params = JSON.parse(new TextDecoder().decode(new Uint8Array(input)));

// TODO: Implement your tool logic here
const output = JSON.stringify({ result: params });

const encoded = new TextEncoder().encode(output);
Javy.IO.writeSync(1, encoded);
"#;
    fs::write(dir.join("tool.js"), tool_js)?;

    let build_sh = format!(
        "#!/usr/bin/env bash\nset -euo pipefail\njavy build tool.js -o {name}.wasm\necho \"Built {name}.wasm\"\n"
    );
    fs::write(dir.join("build.sh"), &build_sh)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dir.join("build.sh"), fs::Permissions::from_mode(0o755))?;
    }

    write_manifest(dir, name)?;
    Ok(())
}

fn scaffold_ts(dir: &Path, name: &str) -> Result<()> {
    fs::create_dir_all(dir)?;

    let tool_ts = r#"// Read input from stdin
const input: number[] = [];
const buf = new Uint8Array(1024);
let n: number;
while ((n = (Javy.IO as any).readSync(0, buf)) > 0) {
    input.push(...buf.subarray(0, n));
}
const params: Record<string, unknown> = JSON.parse(new TextDecoder().decode(new Uint8Array(input)));

// TODO: Implement your tool logic here
const output: string = JSON.stringify({ result: params });

const encoded = new TextEncoder().encode(output);
(Javy.IO as any).writeSync(1, encoded);
"#;
    fs::write(dir.join("tool.ts"), tool_ts)?;

    let build_sh = format!(
        "#!/usr/bin/env bash\nset -euo pipefail\nesbuild tool.ts --bundle --outfile=tool.bundle.js\njavy build tool.bundle.js -o {name}.wasm\nrm -f tool.bundle.js\necho \"Built {name}.wasm\"\n"
    );
    fs::write(dir.join("build.sh"), &build_sh)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dir.join("build.sh"), fs::Permissions::from_mode(0o755))?;
    }

    let tsconfig = r#"{
  "compilerOptions": {
    "target": "es2020",
    "module": "es2020",
    "strict": true,
    "esModuleInterop": true
  },
  "include": ["*.ts"]
}
"#;
    fs::write(dir.join("tsconfig.json"), tsconfig)?;
    write_manifest(dir, name)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_scaffold_rust() {
        let tmp = TempDir::new().unwrap();
        scaffold("my-tool", "rust", Some(tmp.path().to_str().unwrap())).unwrap();
        let dir = tmp.path().join("my-tool");
        assert!(dir.join("Cargo.toml").exists());
        assert!(dir.join("src/main.rs").exists());
        assert!(dir.join("manifest.json").exists());
    }

    #[test]
    fn test_scaffold_js() {
        let tmp = TempDir::new().unwrap();
        scaffold("my-tool", "js", Some(tmp.path().to_str().unwrap())).unwrap();
        let dir = tmp.path().join("my-tool");
        assert!(dir.join("tool.js").exists());
        assert!(dir.join("build.sh").exists());
        assert!(dir.join("manifest.json").exists());
    }

    #[test]
    fn test_scaffold_ts() {
        let tmp = TempDir::new().unwrap();
        scaffold("my-tool", "ts", Some(tmp.path().to_str().unwrap())).unwrap();
        let dir = tmp.path().join("my-tool");
        assert!(dir.join("tool.ts").exists());
        assert!(dir.join("build.sh").exists());
        assert!(dir.join("tsconfig.json").exists());
        assert!(dir.join("manifest.json").exists());
    }

    #[test]
    fn test_scaffold_invalid_lang() {
        let tmp = TempDir::new().unwrap();
        let result = scaffold("x", "python", Some(tmp.path().to_str().unwrap()));
        assert!(result.is_err());
    }

    #[test]
    fn test_scaffold_existing_dir() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir(tmp.path().join("exists")).unwrap();
        let result = scaffold("exists", "rust", Some(tmp.path().to_str().unwrap()));
        assert!(result.is_err());
    }
}
