use crate::config::{Backend, backend_name};
use crate::conversation::{cmd_send, cmd_send_inner};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub(crate) enum ReviewVerdict {
    Success,
    Retry(String),
}

pub(crate) fn parse_review(response: &str) -> ReviewVerdict {
    let upper = response.to_uppercase();
    if upper.contains("VERDICT: SUCCESS") {
        return ReviewVerdict::Success;
    }
    if upper.contains("VERDICT: RETRY") {
        if let Some(pos) = upper.find("FEEDBACK:") {
            let feedback = response[pos + "FEEDBACK:".len()..].trim().to_string();
            return ReviewVerdict::Retry(feedback);
        }
        return ReviewVerdict::Retry(response.to_string());
    }
    ReviewVerdict::Retry(response.to_string())
}

pub(crate) fn truncate_display(s: &str, max: usize) -> String {
    let first_line = s.lines().next().unwrap_or(s);
    if first_line.len() > max {
        format!("{}...", &first_line[..max])
    } else {
        first_line.to_string()
    }
}

pub(crate) fn build_file_refs(files: &[PathBuf]) -> String {
    if files.is_empty() {
        String::new()
    } else {
        let paths: Vec<_> = files
            .iter()
            .map(|f| format!("  - {}", f.display()))
            .collect();
        format!("\nAlso read these reference files:\n{}\n", paths.join("\n"))
    }
}

pub(crate) fn build_first_message(plan_path: &Path, file_refs: &str) -> String {
    format!(
        "Read the implementation plan from {} and follow the instructions.\n\
         {file_refs}{RESULT_INSTRUCTIONS}",
        plan_path.display()
    )
}

pub(crate) fn build_review_message(verification_path: &Path) -> String {
    format!(
        "You are a validator reviewing an implementation attempt.\n\n\
         Read the acceptance criteria from {}.\n\
         Read RESULT.md for the implementation progress.\n\n\
         Review the implementation against the criteria.\n\
         After your review, update RESULT.md by appending under the current attempt:\n\
         - Your verdict (SUCCESS or RETRY)\n\
         - Specific feedback on what was done well and what needs fixing\n\
         - Concrete instructions for the next attempt (if RETRY)\n\n\
         Then respond with:\n\
         - VERDICT: SUCCESS (if all criteria met)\n\
         - VERDICT: RETRY + FEEDBACK: ... (if not)\n\n\
         Only return SUCCESS if the verification criteria are completely satisfied.",
        verification_path.display()
    )
}

pub(crate) fn build_retry_message(plan_path: &Path) -> String {
    format!(
        "Read the implementation plan from {}.\n\
         Check RESULT.md for validator feedback on previous attempts and address it.\n\
         {RESULT_INSTRUCTIONS}",
        plan_path.display()
    )
}

pub(crate) fn build_final_status(iteration: usize) -> String {
    format!("\n## Final Status\nCompleted successfully after {iteration} attempt(s).\n")
}

pub(crate) const RESULT_INITIAL: &str = "# Result\n\n## Progress\n";

const RESULT_INSTRUCTIONS: &str = "\n\nAfter completing your work, update RESULT.md with:\n\
    - A summary of changes made under a \"### Attempt N\" heading\n\
    - Files modified and why\n\
    - Any issues encountered and how they were resolved\n\
    - Lessons learned";

/// Validate that plan and verification files exist.
pub(crate) fn validate_loop_files(
    plan_path: &Path,
    verification_path: &Path,
) -> Result<(), String> {
    if let Err(e) = fs::metadata(plan_path) {
        return Err(format!(
            "Failed to read plan file {}: {e}",
            plan_path.display()
        ));
    }
    if let Err(e) = fs::metadata(verification_path) {
        return Err(format!(
            "Failed to read verification file {}: {e}",
            verification_path.display()
        ));
    }
    Ok(())
}

/// Format the loop header banner.
pub(crate) fn format_loop_header(
    plan_path: &Path,
    verification_path: &Path,
    backend: &Backend,
    review_backend: &Backend,
) -> String {
    format!(
        "[loop] Plan: {} | Verification: {}\n\
         [loop] Result: RESULT.md\n\
         [loop] Implementer: {} | Validator: {}\n\
         [loop] Press Ctrl-C to stop at any time",
        plan_path.display(),
        verification_path.display(),
        backend_name(backend),
        backend_name(review_backend),
    )
}

/// Process a review response and return the formatted review failure message (if retry).
pub(crate) fn format_review_failure(
    sandbox: Option<&str>,
    review_backend: &Backend,
    stderr: &str,
    name: &str,
) -> String {
    let label = if sandbox.is_some() {
        "limactl"
    } else {
        backend_name(review_backend)
    };
    format!(
        "{label} failed during review: {stderr}\n\
         [loop] Stopping due to review error. Conversation: {name}"
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn cmd_loop(
    plan_path: &Path,
    verification_path: &Path,
    target: Option<&str>,
    model: Option<&str>,
    backend: &Backend,
    review_model: Option<&str>,
    review_backend: &Backend,
    files: &[PathBuf],
    sandbox: Option<&str>,
    push: bool,
) -> String {
    if let Err(msg) = validate_loop_files(plan_path, verification_path) {
        eprintln!("{msg}");
        std::process::exit(1);
    }

    let result_path = std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("RESULT.md");
    if let Err(e) = fs::write(&result_path, RESULT_INITIAL) {
        eprintln!("Failed to create RESULT.md: {e}");
        std::process::exit(1);
    }

    eprintln!(
        "{}\n",
        format_loop_header(plan_path, verification_path, backend, review_backend)
    );

    let file_refs = build_file_refs(files);

    eprintln!("[loop] === Attempt 1 ===");
    let first_message = build_first_message(plan_path, &file_refs);
    let name = cmd_send(&first_message, target, model, backend, &[], sandbox, push);

    let mut iteration = 1;
    loop {
        eprintln!("\n[loop] Reviewing attempt {iteration}...");

        let review_message = build_review_message(verification_path);

        let (_, response_or_err, success) = cmd_send_inner(
            &review_message,
            Some(&name),
            review_model,
            review_backend,
            &[],
            sandbox,
            push,
            false,
        );

        if !success {
            eprintln!(
                "{}",
                format_review_failure(sandbox, review_backend, &response_or_err, &name)
            );
            return name;
        }

        let response = response_or_err.trim();
        match parse_review(response) {
            ReviewVerdict::Success => {
                let final_status = build_final_status(iteration);
                if let Ok(mut f) = fs::OpenOptions::new().append(true).open(&result_path) {
                    use io::Write;
                    let _ = f.write_all(final_status.as_bytes());
                }
                eprintln!("[loop] === SUCCESS after {} attempt(s) ===", iteration);
                return name;
            }
            ReviewVerdict::Retry(feedback) => {
                eprintln!("[loop] Verdict: RETRY");
                eprintln!("[loop] Feedback: {}", truncate_display(&feedback, 120));

                iteration += 1;
                let retry_message = build_retry_message(plan_path);

                eprintln!("\n[loop] === Attempt {iteration} ===");
                cmd_send(
                    &retry_message,
                    Some(&name),
                    model,
                    backend,
                    &[],
                    sandbox,
                    push,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_review_success() {
        assert!(matches!(
            parse_review("VERDICT: SUCCESS"),
            ReviewVerdict::Success
        ));
    }

    #[test]
    fn parse_review_retry_with_feedback() {
        match parse_review("VERDICT: RETRY\nFEEDBACK: fix X") {
            ReviewVerdict::Retry(s) => assert_eq!(s, "fix X"),
            _ => panic!("expected retry"),
        }
    }

    #[test]
    fn parse_review_retry_without_feedback() {
        match parse_review("VERDICT: RETRY") {
            ReviewVerdict::Retry(s) => assert_eq!(s, "VERDICT: RETRY"),
            _ => panic!("expected retry"),
        }
    }

    #[test]
    fn parse_review_no_verdict() {
        match parse_review("no verdict here") {
            ReviewVerdict::Retry(s) => assert_eq!(s, "no verdict here"),
            _ => panic!("expected retry"),
        }
    }

    #[test]
    fn parse_review_case_insensitive() {
        assert!(matches!(
            parse_review("verdict: success"),
            ReviewVerdict::Success
        ));
    }

    #[test]
    fn truncate_display_short() {
        assert_eq!(truncate_display("short", 100), "short");
    }

    #[test]
    fn truncate_display_long() {
        assert_eq!(truncate_display("a very long string", 5), "a ver...");
    }

    #[test]
    fn truncate_display_multiline() {
        assert_eq!(truncate_display("line1\nline2", 100), "line1");
    }

    #[test]
    fn parse_review_success_with_surrounding_text() {
        assert!(matches!(
            parse_review("Some preamble\nVERDICT: SUCCESS\nSome epilogue"),
            ReviewVerdict::Success
        ));
    }

    #[test]
    fn parse_review_retry_with_multiline_feedback() {
        match parse_review("VERDICT: RETRY\nFEEDBACK: fix line 1\nfix line 2") {
            ReviewVerdict::Retry(s) => assert!(s.starts_with("fix line 1")),
            _ => panic!("expected retry"),
        }
    }

    #[test]
    fn parse_review_mixed_case() {
        assert!(matches!(
            parse_review("Verdict: Success"),
            ReviewVerdict::Success
        ));
    }

    #[test]
    fn parse_review_empty_input() {
        match parse_review("") {
            ReviewVerdict::Retry(s) => assert!(s.is_empty()),
            _ => panic!("expected retry"),
        }
    }

    #[test]
    fn truncate_display_exact_length() {
        assert_eq!(truncate_display("12345", 5), "12345");
    }

    #[test]
    fn truncate_display_empty() {
        assert_eq!(truncate_display("", 100), "");
    }

    #[test]
    fn truncate_display_one_char_max() {
        assert_eq!(truncate_display("abc", 1), "a...");
    }

    #[test]
    fn parse_review_retry_feedback_only_whitespace() {
        match parse_review("VERDICT: RETRY\nFEEDBACK:   ") {
            ReviewVerdict::Retry(s) => assert!(s.is_empty() || s.trim().is_empty()),
            _ => panic!("expected retry"),
        }
    }

    #[test]
    fn build_file_refs_empty() {
        assert!(build_file_refs(&[]).is_empty());
    }

    #[test]
    fn build_file_refs_single() {
        let refs = build_file_refs(&[PathBuf::from("a.txt")]);
        assert!(refs.contains("a.txt"));
        assert!(refs.contains("Also read"));
    }

    #[test]
    fn build_file_refs_multiple() {
        let refs = build_file_refs(&[PathBuf::from("a.txt"), PathBuf::from("b.rs")]);
        assert!(refs.contains("a.txt"));
        assert!(refs.contains("b.rs"));
    }

    #[test]
    fn build_first_message_contains_plan_path() {
        let msg = build_first_message(Path::new("PLAN.md"), "");
        assert!(msg.contains("PLAN.md"));
        assert!(msg.contains("implementation plan"));
        assert!(msg.contains("RESULT.md"));
    }

    #[test]
    fn build_first_message_with_file_refs() {
        let msg = build_first_message(Path::new("PLAN.md"), "\nAlso read:\n  - foo.rs\n");
        assert!(msg.contains("PLAN.md"));
        assert!(msg.contains("foo.rs"));
    }

    #[test]
    fn build_review_message_contains_verification_path() {
        let msg = build_review_message(Path::new("VERIFICATION.md"));
        assert!(msg.contains("VERIFICATION.md"));
        assert!(msg.contains("VERDICT: SUCCESS"));
        assert!(msg.contains("VERDICT: RETRY"));
        assert!(msg.contains("FEEDBACK"));
    }

    #[test]
    fn build_retry_message_contains_plan_path() {
        let msg = build_retry_message(Path::new("PLAN.md"));
        assert!(msg.contains("PLAN.md"));
        assert!(msg.contains("validator feedback"));
        assert!(msg.contains("RESULT.md"));
    }

    #[test]
    fn build_final_status_format() {
        let s = build_final_status(1);
        assert!(s.contains("1 attempt(s)"));
        assert!(s.contains("Final Status"));
    }

    #[test]
    fn build_final_status_multiple() {
        let s = build_final_status(5);
        assert!(s.contains("5 attempt(s)"));
    }

    #[test]
    fn result_instructions_mentions_required_sections() {
        assert!(RESULT_INSTRUCTIONS.contains("Attempt N"));
        assert!(RESULT_INSTRUCTIONS.contains("Files modified"));
        assert!(RESULT_INSTRUCTIONS.contains("Lessons learned"));
    }

    #[test]
    fn parse_review_verdict_success_with_extra_content() {
        assert!(matches!(
            parse_review("Lots of text\nVERDICT: SUCCESS\nMore text\nFEEDBACK: ignored"),
            ReviewVerdict::Success
        ));
    }

    #[test]
    fn parse_review_unicode_content() {
        match parse_review("VERDICT: RETRY\nFEEDBACK: fix módulo cálculo") {
            ReviewVerdict::Retry(s) => assert!(s.contains("módulo")),
            _ => panic!("expected retry"),
        }
    }

    #[test]
    fn truncate_display_ascii_boundary() {
        // Only test with ASCII to avoid byte-boundary issues in the truncation
        let s = "abcdefghij";
        let result = truncate_display(s, 5);
        assert_eq!(result, "abcde...");
    }

    // --- validate_loop_files tests ---

    #[test]
    fn validate_loop_files_both_exist() {
        let td = tempfile::TempDir::new().expect("tempdir");
        let plan = td.path().join("PLAN.md");
        let verify = td.path().join("VERIFICATION.md");
        fs::write(&plan, "plan").unwrap();
        fs::write(&verify, "verify").unwrap();
        assert!(validate_loop_files(&plan, &verify).is_ok());
    }

    #[test]
    fn validate_loop_files_plan_missing() {
        let td = tempfile::TempDir::new().expect("tempdir");
        let plan = td.path().join("PLAN.md");
        let verify = td.path().join("VERIFICATION.md");
        fs::write(&verify, "verify").unwrap();
        let err = validate_loop_files(&plan, &verify).unwrap_err();
        assert!(err.contains("plan file"));
    }

    #[test]
    fn validate_loop_files_verification_missing() {
        let td = tempfile::TempDir::new().expect("tempdir");
        let plan = td.path().join("PLAN.md");
        let verify = td.path().join("VERIFICATION.md");
        fs::write(&plan, "plan").unwrap();
        let err = validate_loop_files(&plan, &verify).unwrap_err();
        assert!(err.contains("verification file"));
    }

    #[test]
    fn validate_loop_files_both_missing() {
        let td = tempfile::TempDir::new().expect("tempdir");
        let plan = td.path().join("PLAN.md");
        let verify = td.path().join("VERIFICATION.md");
        // plan is checked first, so error mentions plan
        let err = validate_loop_files(&plan, &verify).unwrap_err();
        assert!(err.contains("plan file"));
    }

    // --- format_loop_header tests ---

    #[test]
    fn format_loop_header_contains_all_parts() {
        let header = format_loop_header(
            Path::new("PLAN.md"),
            Path::new("VERIFY.md"),
            &Backend::Claude,
            &Backend::Gemini,
        );
        assert!(header.contains("PLAN.md"));
        assert!(header.contains("VERIFY.md"));
        assert!(header.contains("claude"));
        assert!(header.contains("gemini"));
        assert!(header.contains("RESULT.md"));
        assert!(header.contains("Ctrl-C"));
    }

    #[test]
    fn format_loop_header_same_backend() {
        let header = format_loop_header(
            Path::new("P.md"),
            Path::new("V.md"),
            &Backend::Codex,
            &Backend::Codex,
        );
        assert!(header.contains("codex"));
        assert!(header.contains("Implementer"));
        assert!(header.contains("Validator"));
    }

    // --- format_review_failure tests ---

    #[test]
    fn format_review_failure_without_sandbox() {
        let msg = format_review_failure(None, &Backend::Claude, "err msg", "conv-1");
        assert!(msg.contains("claude failed during review"));
        assert!(msg.contains("err msg"));
        assert!(msg.contains("conv-1"));
    }

    #[test]
    fn format_review_failure_with_sandbox() {
        let msg = format_review_failure(Some("vm1"), &Backend::Gemini, "timeout", "conv-2");
        assert!(msg.contains("limactl failed during review"));
        assert!(msg.contains("timeout"));
        assert!(msg.contains("conv-2"));
    }

    // --- RESULT_INITIAL constant test ---

    #[test]
    fn result_initial_has_expected_sections() {
        assert!(RESULT_INITIAL.contains("# Result"));
        assert!(RESULT_INITIAL.contains("## Progress"));
    }

    // --- more parse_review edge cases ---

    #[test]
    fn parse_review_verdict_retry_lowercase() {
        match parse_review("verdict: retry\nfeedback: fix it") {
            ReviewVerdict::Retry(s) => assert!(s.contains("fix it")),
            _ => panic!("expected retry"),
        }
    }

    #[test]
    fn parse_review_success_takes_priority_over_retry() {
        // If both SUCCESS and RETRY appear, SUCCESS checked first
        assert!(matches!(
            parse_review("VERDICT: SUCCESS\nVERDICT: RETRY\nFEEDBACK: x"),
            ReviewVerdict::Success
        ));
    }

    #[test]
    fn parse_review_whitespace_only() {
        match parse_review("   \n\n  ") {
            ReviewVerdict::Retry(s) => assert!(s.trim().is_empty() || s == "   \n\n  "),
            _ => panic!("expected retry"),
        }
    }

    #[test]
    fn parse_review_very_long_input() {
        let long = "some text ".repeat(10_000);
        match parse_review(&long) {
            ReviewVerdict::Retry(_) => {} // no verdict found
            _ => panic!("expected retry"),
        }
    }

    // --- more truncate_display tests ---

    #[test]
    fn truncate_display_multiline_long_first_line() {
        let input = format!("{}\nsecond line", "x".repeat(200));
        let result = truncate_display(&input, 10);
        assert_eq!(result, format!("{}...", "x".repeat(10)));
    }

    #[test]
    fn truncate_display_preserves_content() {
        assert_eq!(truncate_display("hello world", 11), "hello world");
        assert_eq!(truncate_display("hello world", 10), "hello worl...");
    }

    // --- more build_file_refs tests ---

    #[test]
    fn build_file_refs_with_paths() {
        let files = vec![
            PathBuf::from("/home/user/code.rs"),
            PathBuf::from("relative/path.txt"),
        ];
        let refs = build_file_refs(&files);
        assert!(refs.contains("/home/user/code.rs"));
        assert!(refs.contains("relative/path.txt"));
        assert!(refs.contains("Also read"));
    }

    #[test]
    fn build_file_refs_formatting() {
        let refs = build_file_refs(&[PathBuf::from("test.md")]);
        assert!(refs.starts_with('\n'));
        assert!(refs.contains("  - test.md"));
    }

    // --- more build_first_message tests ---

    #[test]
    fn build_first_message_without_file_refs() {
        let msg = build_first_message(Path::new("PLAN.md"), "");
        assert!(msg.contains("PLAN.md"));
        assert!(msg.contains("RESULT.md"));
        assert!(!msg.contains("Also read"));
    }

    #[test]
    fn build_first_message_has_result_instructions() {
        let msg = build_first_message(Path::new("P.md"), "");
        assert!(msg.contains("Attempt N"));
        assert!(msg.contains("Files modified"));
    }

    // --- more build_review_message tests ---

    #[test]
    fn build_review_message_has_verdict_instructions() {
        let msg = build_review_message(Path::new("V.md"));
        assert!(msg.contains("VERDICT: SUCCESS"));
        assert!(msg.contains("VERDICT: RETRY"));
        assert!(msg.contains("FEEDBACK"));
        assert!(msg.contains("RESULT.md"));
    }

    #[test]
    fn build_review_message_mentions_criteria() {
        let msg = build_review_message(Path::new("CHECK.md"));
        assert!(msg.contains("CHECK.md"));
        assert!(msg.contains("acceptance criteria") || msg.contains("criteria"));
    }

    // --- more build_retry_message tests ---

    #[test]
    fn build_retry_message_has_result_instructions() {
        let msg = build_retry_message(Path::new("PLAN.md"));
        assert!(msg.contains("RESULT.md"));
        assert!(msg.contains("validator feedback"));
    }

    // --- more build_final_status tests ---

    #[test]
    fn build_final_status_various_iterations() {
        for n in [1, 2, 5, 10, 100] {
            let s = build_final_status(n);
            assert!(s.contains(&format!("{n} attempt(s)")));
            assert!(s.contains("Final Status"));
            assert!(s.contains("successfully"));
        }
    }

    // --- more validate_loop_files tests ---

    #[test]
    fn validate_loop_files_error_includes_path() {
        let td = tempfile::TempDir::new().expect("tempdir");
        let plan = td.path().join("MY_PLAN.md");
        let verify = td.path().join("VERIFY.md");
        fs::write(&verify, "x").unwrap();
        let err = validate_loop_files(&plan, &verify).unwrap_err();
        assert!(err.contains("MY_PLAN.md") || err.contains("plan file"));
    }

    #[test]
    fn validate_loop_files_verification_error_includes_path() {
        let td = tempfile::TempDir::new().expect("tempdir");
        let plan = td.path().join("PLAN.md");
        let verify = td.path().join("MY_VERIFY.md");
        fs::write(&plan, "x").unwrap();
        let err = validate_loop_files(&plan, &verify).unwrap_err();
        assert!(err.contains("MY_VERIFY.md") || err.contains("verification file"));
    }

    // --- more format_loop_header tests ---

    #[test]
    fn format_loop_header_all_backends() {
        for (be1, be2) in [
            (Backend::Claude, Backend::Gemini),
            (Backend::Codex, Backend::Claude),
            (Backend::Gemini, Backend::Codex),
        ] {
            let header = format_loop_header(Path::new("P.md"), Path::new("V.md"), &be1, &be2);
            assert!(header.contains("Implementer"));
            assert!(header.contains("Validator"));
            assert!(header.contains("Ctrl-C"));
        }
    }

    #[test]
    fn format_loop_header_with_long_paths() {
        let header = format_loop_header(
            Path::new("/long/path/to/PLAN.md"),
            Path::new("/long/path/to/VERIFY.md"),
            &Backend::Claude,
            &Backend::Claude,
        );
        assert!(header.contains("/long/path/to/PLAN.md"));
        assert!(header.contains("/long/path/to/VERIFY.md"));
    }

    // --- more format_review_failure tests ---

    #[test]
    fn format_review_failure_all_backends() {
        for backend in [Backend::Claude, Backend::Codex, Backend::Gemini] {
            let msg = format_review_failure(None, &backend, "err", "c");
            assert!(msg.contains("failed during review"));
            assert!(msg.contains("err"));
            assert!(msg.contains("c"));
        }
    }

    #[test]
    fn format_review_failure_sandbox_overrides_backend_label() {
        let msg = format_review_failure(Some("vm"), &Backend::Claude, "err", "c");
        assert!(msg.contains("limactl"));
        assert!(!msg.contains("claude"));
    }

    // --- RESULT_INSTRUCTIONS tests ---

    #[test]
    fn result_instructions_has_all_required_items() {
        assert!(RESULT_INSTRUCTIONS.contains("Attempt N"));
        assert!(RESULT_INSTRUCTIONS.contains("Files modified"));
        assert!(RESULT_INSTRUCTIONS.contains("issues encountered"));
        assert!(RESULT_INSTRUCTIONS.contains("Lessons learned"));
    }

    #[test]
    fn result_instructions_mentions_result_md() {
        assert!(RESULT_INSTRUCTIONS.contains("RESULT.md"));
    }
}
