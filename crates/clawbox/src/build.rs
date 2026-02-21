use anyhow::{Context, Result, bail};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug)]
enum Lang {
    Rust,
    JavaScript,
    TypeScript,
}

#[derive(Debug)]
struct ToolProject {
    name: String,
    path: PathBuf,
    lang: Lang,
}

fn detect_lang(dir: &Path) -> Option<Lang> {
    if dir.join("Cargo.toml").exists() {
        return Some(Lang::Rust);
    }
    let has_ts = fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .any(|e| e.path().extension().is_some_and(|ext| ext == "ts"));
    if has_ts {
        return Some(Lang::TypeScript);
    }
    let has_js = fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .any(|e| e.path().extension().is_some_and(|ext| ext == "js"));
    if has_js {
        return Some(Lang::JavaScript);
    }
    None
}

fn find_tools(source: &Path) -> Result<Vec<ToolProject>> {
    let canonical_source = fs::canonicalize(source)
        .with_context(|| format!("Failed to canonicalize source path: {}", source.display()))?;
    let mut tools = Vec::new();

    fn scan(dir: &Path, canonical_source: &Path, tools: &mut Vec<ToolProject>) -> Result<()> {
        if let Some(lang) = detect_lang(dir) {
            let canonical_dir = match fs::canonicalize(dir) {
                Ok(d) => d,
                Err(_) => return Ok(()),
            };
            if !canonical_dir.starts_with(canonical_source) {
                return Ok(());
            }
            let name = match dir.file_name().map(|n| n.to_string_lossy().to_string()) {
                Some(n) => n,
                None => return Ok(()),
            };
            tools.push(ToolProject {
                name,
                path: dir.to_path_buf(),
                lang,
            });
            return Ok(());
        }
        // Recurse into subdirectories
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.filter_map(|e| e.ok()) {
                let p = entry.path();
                if p.is_dir() {
                    scan(&p, canonical_source, tools)?;
                }
            }
        }
        Ok(())
    }

    scan(&canonical_source, &canonical_source, &mut tools)?;
    Ok(tools)
}

fn check_tool(name: &str) -> Result<()> {
    Command::new("which")
        .arg(name)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .with_context(|| format!("Required tool '{name}' not found in PATH"))?;
    Ok(())
}

fn build_rust(project: &ToolProject, output: &Path) -> Result<()> {
    check_tool("cargo")?;
    let status = Command::new("cargo")
        .args(["build", "--target", "wasm32-wasip1", "--release"])
        .current_dir(&project.path)
        .status()
        .context("Failed to run cargo")?;
    if !status.success() {
        bail!("cargo build failed for '{}'", project.name);
    }

    // Find .wasm files in target dir
    let target_dir = project.path.join("target/wasm32-wasip1/release");
    if let Ok(entries) = fs::read_dir(&target_dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let p = entry.path();
            if p.extension().is_some_and(|e| e == "wasm") {
                let dest = output.join(p.file_name().unwrap());
                fs::copy(&p, &dest)?;
                eprintln!("  Copied {} → {}", p.display(), dest.display());
            }
        }
    }
    Ok(())
}

fn build_js(project: &ToolProject, output: &Path) -> Result<()> {
    check_tool("javy")?;
    let main = find_main_file(&project.path, "js")?;
    let out_wasm = output.join(format!("{}.wasm", project.name));
    let status = Command::new("javy")
        .args([
            "build",
            &main.to_string_lossy(),
            "-o",
            &out_wasm.to_string_lossy(),
        ])
        .current_dir(&project.path)
        .status()
        .context("Failed to run javy")?;
    if !status.success() {
        bail!("javy build failed for '{}'", project.name);
    }
    Ok(())
}

fn build_ts(project: &ToolProject, output: &Path) -> Result<()> {
    check_tool("esbuild")?;
    check_tool("javy")?;
    let main = find_main_file(&project.path, "ts")?;
    let tmp_js = std::env::temp_dir().join(format!("clawbox-build-{}.js", project.name));
    let status = Command::new("esbuild")
        .args([
            main.to_string_lossy().as_ref(),
            "--bundle",
            &format!("--outfile={}", tmp_js.display()),
        ])
        .current_dir(&project.path)
        .status()
        .context("Failed to run esbuild")?;
    if !status.success() {
        bail!("esbuild failed for '{}'", project.name);
    }
    let out_wasm = output.join(format!("{}.wasm", project.name));
    let status = Command::new("javy")
        .args([
            "build",
            &tmp_js.to_string_lossy(),
            "-o",
            &out_wasm.to_string_lossy(),
        ])
        .status()
        .context("Failed to run javy")?;
    let _ = fs::remove_file(&tmp_js);
    if !status.success() {
        bail!("javy build failed for '{}'", project.name);
    }
    Ok(())
}

fn find_main_file(dir: &Path, ext: &str) -> Result<PathBuf> {
    // Prefer tool.<ext>, then any *.<ext>
    let tool_file = dir.join(format!("tool.{ext}"));
    if tool_file.exists() {
        return Ok(tool_file);
    }
    let first = fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|e2| e2 == ext));
    first.with_context(|| format!("No .{ext} file found in {}", dir.display()))
}

pub fn build(tool: Option<&str>, source: &str, output: &str) -> Result<()> {
    let source = Path::new(source);
    let output = Path::new(output);

    if !source.exists() {
        bail!("Source directory not found: {}", source.display());
    }
    fs::create_dir_all(output)?;

    let all_tools = find_tools(source)?;
    let tools: Vec<_> = match tool {
        Some(name) => {
            let found: Vec<_> = all_tools.into_iter().filter(|t| t.name == name).collect();
            if found.is_empty() {
                bail!("Tool '{name}' not found in {}", source.display());
            }
            found
        }
        None => all_tools,
    };

    if tools.is_empty() {
        eprintln!("No tools found in {}", source.display());
        return Ok(());
    }

    let mut ok = 0usize;
    let mut fail = 0usize;

    for t in &tools {
        eprintln!("Building {:?} tool '{}'...", t.lang, t.name);
        let result = match t.lang {
            Lang::Rust => build_rust(t, output),
            Lang::JavaScript => build_js(t, output),
            Lang::TypeScript => build_ts(t, output),
        };
        match result {
            Ok(()) => {
                eprintln!("  ✓ {}", t.name);
                ok += 1;
            }
            Err(e) => {
                eprintln!("  ✗ {}: {e}", t.name);
                fail += 1;
            }
        }
    }

    eprintln!("\nBuild complete: {ok} succeeded, {fail} failed");
    if fail > 0 {
        bail!("{fail} tool(s) failed to build");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_detect_lang_rust() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("Cargo.toml"), "").unwrap();
        assert!(matches!(detect_lang(tmp.path()), Some(Lang::Rust)));
    }

    #[test]
    fn test_detect_lang_js() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("tool.js"), "").unwrap();
        assert!(matches!(detect_lang(tmp.path()), Some(Lang::JavaScript)));
    }

    #[test]
    fn test_detect_lang_ts() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("tool.ts"), "").unwrap();
        assert!(matches!(detect_lang(tmp.path()), Some(Lang::TypeScript)));
    }

    #[test]
    fn test_detect_lang_none() {
        let tmp = TempDir::new().unwrap();
        assert!(detect_lang(tmp.path()).is_none());
    }

    #[test]
    fn test_find_tools() {
        let tmp = TempDir::new().unwrap();
        let rust_dir = tmp.path().join("rust/echo");
        fs::create_dir_all(&rust_dir).unwrap();
        fs::write(rust_dir.join("Cargo.toml"), "").unwrap();
        let js_dir = tmp.path().join("js/hello");
        fs::create_dir_all(&js_dir).unwrap();
        fs::write(js_dir.join("tool.js"), "").unwrap();
        let tools = find_tools(tmp.path()).unwrap();
        assert_eq!(tools.len(), 2);
    }

    #[test]
    fn test_build_missing_source() {
        let result = build(None, "/nonexistent/path", "/tmp/out");
        assert!(result.is_err());
    }
}
