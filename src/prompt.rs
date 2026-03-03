use chrono::{SecondsFormat, Utc};
use clap_complete::engine::CompletionCandidate;
use serde::{Deserialize, Serialize};
use skim::prelude::*;
use std::fs;
use std::io::{self, Cursor};
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
pub(crate) struct PromptFile {
    pub(crate) prompt: Vec<PromptEntry>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub(crate) struct PromptEntry {
    pub(crate) name: String,
    pub(crate) body: String,
    pub(crate) updated_at: String,
}

pub(crate) fn prompts_file_path() -> PathBuf {
    crate::conversation::breo_dir().join("prompts.toml")
}

pub(crate) fn load_prompts() -> Result<Vec<PromptEntry>, String> {
    let path = prompts_file_path();
    let contents = match fs::read_to_string(&path) {
        Ok(v) => v,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(format!("Failed to read {}: {e}", path.display())),
    };

    let parsed: PromptFile = toml::from_str(&contents)
        .map_err(|e| format!("Failed to parse {}: {e}", path.display()))?;
    Ok(parsed.prompt)
}

pub(crate) fn save_prompts(entries: &[PromptEntry]) -> Result<(), String> {
    let path = prompts_file_path();
    let prompt_file = PromptFile {
        prompt: entries.to_vec(),
    };
    let contents =
        toml::to_string(&prompt_file).map_err(|e| format!("Failed to serialize prompts: {e}"))?;
    fs::write(&path, contents).map_err(|e| format!("Failed to write {}: {e}", path.display()))?;
    Ok(())
}

pub(crate) fn list_prompts() -> Vec<CompletionCandidate> {
    let Ok(entries) = load_prompts() else {
        return vec![];
    };
    let mut names: Vec<String> = entries.into_iter().map(|e| e.name).collect();
    names.sort();
    names.into_iter().map(CompletionCandidate::new).collect()
}

pub(crate) fn prompt_body_by_name(name: &str) -> Result<String, String> {
    let entries = load_prompts()?;
    entries
        .into_iter()
        .find(|e| e.name == name)
        .map(|e| e.body)
        .ok_or_else(|| format!("Prompt '{name}' not found."))
}

pub(crate) fn resolve_editor_command() -> String {
    std::env::var("VISUAL")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .or_else(|| {
            std::env::var("EDITOR")
                .ok()
                .filter(|v| !v.trim().is_empty())
        })
        .unwrap_or_else(|| "vi".to_string())
}

pub(crate) fn normalize_editor_content(content: &str) -> String {
    if let Some(stripped) = content.strip_suffix("\r\n") {
        stripped.to_string()
    } else if let Some(stripped) = content.strip_suffix('\n') {
        stripped.to_string()
    } else {
        content.to_string()
    }
}

pub(crate) fn prompt_body_preview(body: &str) -> String {
    let one_line = body.replace(['\n', '\t'], " ");
    let trimmed = one_line.trim();
    if trimmed.len() <= 80 {
        trimmed.to_string()
    } else {
        format!("{}...", &trimmed[..77])
    }
}

pub(crate) fn picker_row(entry: &PromptEntry, index: usize) -> String {
    let preview = prompt_body_preview(&entry.body);
    let searchable = entry.body.replace(['\n', '\t'], " ");
    format!("{}\t{}\t{}\t{}", entry.name, preview, searchable, index)
}

pub(crate) fn picker_index_from_row(row: &str) -> Option<usize> {
    row.rsplit_once('\t')?.1.trim().parse::<usize>().ok()
}

fn now_timestamp() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn find_prompt_index(entries: &[PromptEntry], name: &str) -> Option<usize> {
    entries.iter().position(|e| e.name == name)
}

fn require_prompt_index(entries: &[PromptEntry], name: &str) -> Result<usize, String> {
    find_prompt_index(entries, name).ok_or_else(|| format!("Prompt '{name}' not found."))
}

fn read_prompt_from_editor(initial: &str) -> Result<String, String> {
    let editor = resolve_editor_command();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let tmp_path =
        std::env::temp_dir().join(format!("breo-prompt-{}-{nanos}.md", std::process::id()));

    fs::write(&tmp_path, initial).map_err(|e| format!("Failed to write temp file: {e}"))?;

    let command = format!("{editor} \"$1\"");
    let status = Command::new("sh")
        .arg("-c")
        .arg(command)
        .arg("sh")
        .arg(&tmp_path)
        .status()
        .map_err(|e| format!("Failed to launch editor '{editor}': {e}"))?;

    if !status.success() {
        let _ = fs::remove_file(&tmp_path);
        return Err(format!("Editor '{editor}' exited with status {status}"));
    }

    let edited = fs::read_to_string(&tmp_path).map_err(|e| {
        let _ = fs::remove_file(&tmp_path);
        format!("Failed to read temp file after edit: {e}")
    })?;
    let _ = fs::remove_file(&tmp_path);

    let normalized = normalize_editor_content(&edited);
    if normalized.trim().is_empty() {
        return Err("Prompt body cannot be empty.".to_string());
    }
    Ok(normalized)
}

fn exit_with_error(message: String) -> ! {
    eprintln!("{message}");
    std::process::exit(1);
}

pub(crate) fn cmd_prompt_save(name: &str, text: Option<&str>) {
    crate::conversation::ensure_breo_dir();

    let mut entries = match load_prompts() {
        Ok(v) => v,
        Err(e) => exit_with_error(e),
    };

    if find_prompt_index(&entries, name).is_some() {
        exit_with_error(format!("Prompt '{name}' already exists."));
    }

    let body = match text {
        Some(t) => {
            if t.trim().is_empty() {
                exit_with_error("Prompt body cannot be empty.".to_string());
            }
            t.to_string()
        }
        None => match read_prompt_from_editor("") {
            Ok(v) => v,
            Err(e) => exit_with_error(e),
        },
    };

    entries.push(PromptEntry {
        name: name.to_string(),
        body,
        updated_at: now_timestamp(),
    });

    if let Err(e) = save_prompts(&entries) {
        exit_with_error(e);
    }
    crate::conversation::git_commit_prompts(&format!("breo: save prompt '{name}'"));
    println!("Saved prompt: {name}");
}

pub(crate) fn cmd_prompt_list() {
    crate::conversation::ensure_breo_dir();
    let entries = match load_prompts() {
        Ok(v) => v,
        Err(e) => exit_with_error(e),
    };
    if entries.is_empty() {
        println!("No prompts yet.");
        return;
    }
    let mut names: Vec<String> = entries.into_iter().map(|e| e.name).collect();
    names.sort();
    for name in names {
        println!("{name}");
    }
}

pub(crate) fn cmd_prompt_edit(name: &str) {
    crate::conversation::ensure_breo_dir();
    let mut entries = match load_prompts() {
        Ok(v) => v,
        Err(e) => exit_with_error(e),
    };

    let index = match require_prompt_index(&entries, name) {
        Ok(v) => v,
        Err(e) => exit_with_error(e),
    };

    let initial = entries[index].body.clone();
    let body = match read_prompt_from_editor(&initial) {
        Ok(v) => v,
        Err(e) => exit_with_error(e),
    };

    entries[index].body = body;
    entries[index].updated_at = now_timestamp();

    if let Err(e) = save_prompts(&entries) {
        exit_with_error(e);
    }
    crate::conversation::git_commit_prompts(&format!("breo: edit prompt '{name}'"));
    println!("Edited prompt: {name}");
}

pub(crate) fn cmd_prompt_delete(name: &str) {
    crate::conversation::ensure_breo_dir();
    let mut entries = match load_prompts() {
        Ok(v) => v,
        Err(e) => exit_with_error(e),
    };

    let index = match require_prompt_index(&entries, name) {
        Ok(v) => v,
        Err(e) => exit_with_error(e),
    };

    entries.remove(index);

    if let Err(e) = save_prompts(&entries) {
        exit_with_error(e);
    }
    crate::conversation::git_commit_prompts(&format!("breo: delete prompt '{name}'"));
    println!("Deleted prompt: {name}");
}

pub(crate) fn cmd_prompt_pick() {
    crate::conversation::ensure_breo_dir();
    let entries = match load_prompts() {
        Ok(v) => v,
        Err(e) => exit_with_error(e),
    };
    if entries.is_empty() {
        std::process::exit(1);
    }

    let mut indexed: Vec<(usize, &PromptEntry)> = entries.iter().enumerate().collect();
    indexed.sort_by(|a, b| a.1.name.cmp(&b.1.name));

    let input = indexed
        .iter()
        .map(|(idx, entry)| picker_row(entry, *idx))
        .collect::<Vec<_>>()
        .join("\n");

    let options = SkimOptionsBuilder::default()
        .prompt("prompt> ".to_string())
        .build()
        .unwrap_or_else(|_| std::process::exit(1));
    let item_reader = SkimItemReader::default();
    let items = item_reader.of_bufread(Cursor::new(input));
    let Ok(output) = Skim::run_with(options, Some(items)) else {
        std::process::exit(1);
    };
    if output.is_abort {
        std::process::exit(1);
    }
    let Some(item) = output.selected_items.first() else {
        std::process::exit(1);
    };
    let Some(index) = picker_index_from_row(&item.output()) else {
        std::process::exit(1);
    };
    let Some(entry) = entries.get(index) else {
        std::process::exit(1);
    };
    print!("{}", entry.body);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use tempfile::TempDir;

    fn with_temp_home<T>(f: impl FnOnce() -> T) -> T {
        let tmp = TempDir::new().expect("tempdir");
        let old_home = std::env::var_os("HOME");
        unsafe {
            std::env::set_var("HOME", tmp.path());
        }
        let out = f();
        unsafe {
            if let Some(v) = old_home {
                std::env::set_var("HOME", v);
            } else {
                std::env::remove_var("HOME");
            }
        }
        out
    }

    #[test]
    #[serial]
    fn load_save_roundtrip() {
        with_temp_home(|| {
            crate::conversation::ensure_breo_dir();
            let entries = vec![PromptEntry {
                name: "greeting".to_string(),
                body: "Say hello".to_string(),
                updated_at: "2026-03-03T20:00:00Z".to_string(),
            }];
            save_prompts(&entries).expect("save");
            let loaded = load_prompts().expect("load");
            assert_eq!(loaded.len(), 1);
            assert_eq!(loaded[0].name, "greeting");
            assert_eq!(loaded[0].body, "Say hello");
        });
    }

    #[test]
    #[serial]
    fn load_missing_file_returns_empty() {
        with_temp_home(|| {
            crate::conversation::ensure_breo_dir();
            let loaded = load_prompts().expect("load");
            assert!(loaded.is_empty());
        });
    }

    #[test]
    #[serial]
    fn duplicate_save_rejected() {
        with_temp_home(|| {
            crate::conversation::ensure_breo_dir();
            let entries = vec![PromptEntry {
                name: "x".to_string(),
                body: "body".to_string(),
                updated_at: "2026-03-03T20:00:00Z".to_string(),
            }];
            save_prompts(&entries).expect("save");
            let loaded = load_prompts().expect("load");
            assert!(find_prompt_index(&loaded, "x").is_some());
            assert!(find_prompt_index(&loaded, "missing").is_none());
        });
    }

    #[test]
    fn delete_missing_prompt_error() {
        let entries = vec![PromptEntry {
            name: "exists".to_string(),
            body: "body".to_string(),
            updated_at: "2026-03-03T20:00:00Z".to_string(),
        }];
        let err = require_prompt_index(&entries, "missing").expect_err("expected missing error");
        assert!(err.contains("Prompt 'missing' not found."));
    }

    #[test]
    fn picker_row_contains_name_body_and_index() {
        let entry = PromptEntry {
            name: "test".to_string(),
            body: "find me in body".to_string(),
            updated_at: "2026-03-03T20:00:00Z".to_string(),
        };
        let row = picker_row(&entry, 7);
        assert!(row.contains("test"));
        assert!(row.contains("find me in body"));
        assert!(row.ends_with("\t7"));
    }

    #[test]
    fn picker_index_extraction() {
        assert_eq!(picker_index_from_row("name\tpreview\tsearch\t12"), Some(12));
        assert_eq!(picker_index_from_row("invalid"), None);
    }

    #[test]
    #[serial]
    fn editor_resolution_visual_takes_priority() {
        let old_visual = std::env::var_os("VISUAL");
        let old_editor = std::env::var_os("EDITOR");
        unsafe {
            std::env::set_var("VISUAL", "code --wait");
            std::env::set_var("EDITOR", "vim");
        }
        let editor = resolve_editor_command();
        unsafe {
            if let Some(v) = old_visual {
                std::env::set_var("VISUAL", v);
            } else {
                std::env::remove_var("VISUAL");
            }
            if let Some(v) = old_editor {
                std::env::set_var("EDITOR", v);
            } else {
                std::env::remove_var("EDITOR");
            }
        }
        assert_eq!(editor, "code --wait");
    }

    #[test]
    #[serial]
    fn editor_resolution_falls_back_to_editor() {
        let old_visual = std::env::var_os("VISUAL");
        let old_editor = std::env::var_os("EDITOR");
        unsafe {
            std::env::remove_var("VISUAL");
            std::env::set_var("EDITOR", "nano");
        }
        let editor = resolve_editor_command();
        unsafe {
            if let Some(v) = old_visual {
                std::env::set_var("VISUAL", v);
            } else {
                std::env::remove_var("VISUAL");
            }
            if let Some(v) = old_editor {
                std::env::set_var("EDITOR", v);
            } else {
                std::env::remove_var("EDITOR");
            }
        }
        assert_eq!(editor, "nano");
    }

    #[test]
    #[serial]
    fn editor_resolution_defaults_to_vi() {
        let old_visual = std::env::var_os("VISUAL");
        let old_editor = std::env::var_os("EDITOR");
        unsafe {
            std::env::remove_var("VISUAL");
            std::env::remove_var("EDITOR");
        }
        let editor = resolve_editor_command();
        unsafe {
            if let Some(v) = old_visual {
                std::env::set_var("VISUAL", v);
            }
            if let Some(v) = old_editor {
                std::env::set_var("EDITOR", v);
            }
        }
        assert_eq!(editor, "vi");
    }

    #[test]
    fn normalize_editor_content_trims_single_newline() {
        assert_eq!(normalize_editor_content("abc\n"), "abc");
        assert_eq!(normalize_editor_content("abc\r\n"), "abc");
        assert_eq!(normalize_editor_content("abc"), "abc");
    }

    #[test]
    fn prompt_body_preview_truncates_long_text() {
        let input = "a".repeat(120);
        let out = prompt_body_preview(&input);
        assert!(out.len() <= 80);
        assert!(out.ends_with("..."));
    }

    #[test]
    #[serial]
    fn list_prompts_candidates_sorted() {
        with_temp_home(|| {
            crate::conversation::ensure_breo_dir();
            let entries = vec![
                PromptEntry {
                    name: "zeta".to_string(),
                    body: "z".to_string(),
                    updated_at: "2026-03-03T20:00:00Z".to_string(),
                },
                PromptEntry {
                    name: "alpha".to_string(),
                    body: "a".to_string(),
                    updated_at: "2026-03-03T20:00:00Z".to_string(),
                },
            ];
            save_prompts(&entries).expect("save");
            let candidates = list_prompts();
            assert_eq!(candidates.len(), 2);
            assert_eq!(candidates[0].get_value(), "alpha");
            assert_eq!(candidates[1].get_value(), "zeta");
        });
    }

    #[test]
    #[serial]
    fn prompt_body_lookup_success_and_missing() {
        with_temp_home(|| {
            crate::conversation::ensure_breo_dir();
            let entries = vec![PromptEntry {
                name: "alpha".to_string(),
                body: "hello world".to_string(),
                updated_at: "2026-03-03T20:00:00Z".to_string(),
            }];
            save_prompts(&entries).expect("save");

            let body = prompt_body_by_name("alpha").expect("lookup existing");
            assert_eq!(body, "hello world");

            let err = prompt_body_by_name("missing").expect_err("missing should fail");
            assert!(err.contains("Prompt 'missing' not found."));
        });
    }

    #[test]
    #[serial]
    fn load_prompts_parse_error() {
        with_temp_home(|| {
            crate::conversation::ensure_breo_dir();
            fs::write(prompts_file_path(), "prompt = [").expect("write broken file");

            let err = load_prompts().expect_err("invalid toml should fail");
            assert!(err.contains("Failed to parse"));
        });
    }

    #[test]
    #[serial]
    fn load_prompts_read_error_when_path_is_directory() {
        with_temp_home(|| {
            crate::conversation::ensure_breo_dir();
            let path = prompts_file_path();
            fs::create_dir_all(&path).expect("create dir at prompt file path");

            let err = load_prompts().expect_err("directory path should fail");
            assert!(err.contains("Failed to read"));
        });
    }

    #[test]
    #[serial]
    fn save_prompts_write_error_when_path_is_directory() {
        with_temp_home(|| {
            crate::conversation::ensure_breo_dir();
            let path = prompts_file_path();
            fs::create_dir_all(&path).expect("create dir at prompt file path");

            let entries = vec![PromptEntry {
                name: "alpha".to_string(),
                body: "hello".to_string(),
                updated_at: "2026-03-03T20:00:00Z".to_string(),
            }];
            let err = save_prompts(&entries).expect_err("write should fail");
            assert!(err.contains("Failed to write"));
        });
    }

    #[test]
    fn now_timestamp_is_rfc3339_utc() {
        let timestamp = now_timestamp();
        assert!(timestamp.ends_with('Z'));
        assert!(chrono::DateTime::parse_from_rfc3339(&timestamp).is_ok());
    }

    #[test]
    #[serial]
    fn read_prompt_from_editor_success_uses_initial_content() {
        let old_visual = std::env::var_os("VISUAL");
        let old_editor = std::env::var_os("EDITOR");
        unsafe {
            std::env::set_var("VISUAL", "true");
            std::env::remove_var("EDITOR");
        }
        let out = read_prompt_from_editor("body from initial").expect("editor should succeed");
        unsafe {
            if let Some(v) = old_visual {
                std::env::set_var("VISUAL", v);
            } else {
                std::env::remove_var("VISUAL");
            }
            if let Some(v) = old_editor {
                std::env::set_var("EDITOR", v);
            } else {
                std::env::remove_var("EDITOR");
            }
        }
        assert_eq!(out, "body from initial");
    }

    #[test]
    #[serial]
    fn read_prompt_from_editor_rejects_empty_content() {
        let old_visual = std::env::var_os("VISUAL");
        let old_editor = std::env::var_os("EDITOR");
        unsafe {
            std::env::set_var("VISUAL", "true");
            std::env::remove_var("EDITOR");
        }
        let err = read_prompt_from_editor("").expect_err("empty content should fail");
        unsafe {
            if let Some(v) = old_visual {
                std::env::set_var("VISUAL", v);
            } else {
                std::env::remove_var("VISUAL");
            }
            if let Some(v) = old_editor {
                std::env::set_var("EDITOR", v);
            } else {
                std::env::remove_var("EDITOR");
            }
        }
        assert!(err.contains("Prompt body cannot be empty."));
    }

    #[test]
    #[serial]
    fn read_prompt_from_editor_reports_nonzero_exit() {
        let old_visual = std::env::var_os("VISUAL");
        let old_editor = std::env::var_os("EDITOR");
        unsafe {
            std::env::set_var("VISUAL", "false");
            std::env::remove_var("EDITOR");
        }
        let err = read_prompt_from_editor("body").expect_err("nonzero editor should fail");
        unsafe {
            if let Some(v) = old_visual {
                std::env::set_var("VISUAL", v);
            } else {
                std::env::remove_var("VISUAL");
            }
            if let Some(v) = old_editor {
                std::env::set_var("EDITOR", v);
            } else {
                std::env::remove_var("EDITOR");
            }
        }
        assert!(err.contains("exited with status"));
    }

    #[test]
    #[serial]
    fn cmd_prompt_save_list_and_delete_happy_path() {
        with_temp_home(|| {
            cmd_prompt_save("alpha", Some("first body"));
            cmd_prompt_save("beta", Some("second body"));
            cmd_prompt_list();

            let entries = load_prompts().expect("load after saves");
            assert_eq!(entries.len(), 2);
            assert!(find_prompt_index(&entries, "alpha").is_some());
            assert!(find_prompt_index(&entries, "beta").is_some());

            cmd_prompt_delete("alpha");
            let entries = load_prompts().expect("load after delete");
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0].name, "beta");
        });
    }

    #[test]
    #[serial]
    fn cmd_prompt_list_handles_empty_store() {
        with_temp_home(|| {
            cmd_prompt_list();
            let entries = load_prompts().expect("load");
            assert!(entries.is_empty());
        });
    }

    #[test]
    #[serial]
    fn cmd_prompt_edit_updates_body() {
        with_temp_home(|| {
            crate::conversation::ensure_breo_dir();
            let entries = vec![PromptEntry {
                name: "editme".to_string(),
                body: "before".to_string(),
                updated_at: "2026-03-03T20:00:00Z".to_string(),
            }];
            save_prompts(&entries).expect("seed prompt");

            let old_visual = std::env::var_os("VISUAL");
            let old_editor = std::env::var_os("EDITOR");
            unsafe {
                std::env::set_var("VISUAL", "sh -c 'printf edited > \"$1\"' x");
                std::env::remove_var("EDITOR");
            }

            cmd_prompt_edit("editme");

            unsafe {
                if let Some(v) = old_visual {
                    std::env::set_var("VISUAL", v);
                } else {
                    std::env::remove_var("VISUAL");
                }
                if let Some(v) = old_editor {
                    std::env::set_var("EDITOR", v);
                } else {
                    std::env::remove_var("EDITOR");
                }
            }

            let loaded = load_prompts().expect("load");
            assert_eq!(loaded.len(), 1);
            assert_eq!(loaded[0].name, "editme");
            assert_eq!(loaded[0].body, "edited");
        });
    }
}
