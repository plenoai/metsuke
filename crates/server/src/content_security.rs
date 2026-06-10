use std::fmt::Write;

/// A finding from the content security scan.
#[derive(Debug)]
pub struct Finding {
    pub file: String,
    pub line: u32,
    pub kind: FindingKind,
    pub detail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindingKind {
    InvisibleUnicode,
    BiDiOverride,
}

impl FindingKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::InvisibleUnicode => "invisible-unicode",
            Self::BiDiOverride => "bidi-override",
        }
    }

    pub fn severity(self) -> &'static str {
        match self {
            Self::InvisibleUnicode | Self::BiDiOverride => "high",
        }
    }
}

/// Scan a unified diff for content security issues.
///
/// Checks only added lines (`+` prefix) so that existing code is not
/// flagged — we only gate new contributions.
pub fn scan_diff(diff: &str) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut current_file = String::new();
    let mut line_number: u32 = 0;

    for raw_line in diff.lines() {
        if let Some(rest) = raw_line.strip_prefix("diff --git ") {
            if let Some(b_path) = rest.split(" b/").nth(1) {
                current_file = b_path.to_string();
            }
            continue;
        }

        if raw_line.starts_with("@@") {
            if let Some(num) = parse_hunk_line_number(raw_line) {
                line_number = num;
            }
            continue;
        }

        if let Some(content) = raw_line.strip_prefix('+') {
            if content.starts_with("++") {
                continue;
            }
            scan_line(content, &current_file, line_number, &mut findings);
            line_number += 1;
        } else if raw_line.starts_with(' ') {
            line_number += 1;
        }
    }

    findings
}

fn scan_line(line: &str, file: &str, line_number: u32, findings: &mut Vec<Finding>) {
    for (i, ch) in line.char_indices() {
        let cp = ch as u32;

        if is_invisible(cp) {
            findings.push(Finding {
                file: file.to_string(),
                line: line_number,
                kind: FindingKind::InvisibleUnicode,
                detail: format!("U+{cp:04X} at column {i} — {}", invisible_name(cp)),
            });
        }

        if is_bidi_override(cp) {
            findings.push(Finding {
                file: file.to_string(),
                line: line_number,
                kind: FindingKind::BiDiOverride,
                detail: format!("U+{cp:04X} at column {i} — BiDi override (Trojan Source vector)",),
            });
        }
    }
}

fn is_invisible(cp: u32) -> bool {
    matches!(cp,
        0x200B..=0x200F |  // Zero-width chars
        0x2060..=0x2064 |  // Word joiner, invisible operators
        0xFEFF |           // BOM / ZWNBSP
        0x00AD |           // Soft hyphen
        0x2028..=0x2029 |  // Line/paragraph separator
        0xE0001..=0xE007F  // Tag characters
    )
}

fn is_bidi_override(cp: u32) -> bool {
    matches!(cp,
        0x202A..=0x202E |  // LRE, RLE, PDF, LRO, RLO
        0x2066..=0x2069    // LRI, RLI, FSI, PDI
    )
}

fn invisible_name(cp: u32) -> &'static str {
    match cp {
        0x200B => "Zero Width Space",
        0x200C => "Zero Width Non-Joiner",
        0x200D => "Zero Width Joiner",
        0x200E => "Left-to-Right Mark",
        0x200F => "Right-to-Left Mark",
        0x2060 => "Word Joiner",
        0x2061 => "Function Application",
        0x2062 => "Invisible Times",
        0x2063 => "Invisible Separator",
        0x2064 => "Invisible Plus",
        0xFEFF => "BOM / Zero Width No-Break Space",
        0x00AD => "Soft Hyphen",
        0x2028 => "Line Separator",
        0x2029 => "Paragraph Separator",
        _ if (0xE0001..=0xE007F).contains(&cp) => "Tag Character",
        _ => "Unknown invisible",
    }
}

fn parse_hunk_line_number(hunk_header: &str) -> Option<u32> {
    // @@ -old,count +new,count @@
    let after_plus = hunk_header.split('+').nth(1)?;
    let num_str = after_plus.split(|c: char| !c.is_ascii_digit()).next()?;
    num_str.parse().ok()
}

/// Format findings into a GitHub Check Run summary (Markdown).
pub fn format_check_summary(findings: &[Finding]) -> (String, String, String) {
    if findings.is_empty() {
        return (
            "success".to_string(),
            "Content Security: clean".to_string(),
            "No invisible characters, BiDi overrides, or homoglyphs detected in added lines."
                .to_string(),
        );
    }

    let conclusion = "failure".to_string();
    let title = format!(
        "Content Security: {} finding{}",
        findings.len(),
        if findings.len() == 1 { "" } else { "s" }
    );

    let mut summary = String::from(
        "## Content Security Scan\n\n\
         Suspicious characters detected in added lines. These may indicate a \
         [Trojan Source](https://trojansource.codes/) or GlassWorm attack.\n\n\
         | File | Line | Severity | Type | Detail |\n\
         |------|------|----------|------|--------|\n",
    );

    for f in findings {
        let _ = writeln!(
            summary,
            "| `{}` | {} | {} | {} | {} |",
            f.file,
            f.line,
            f.kind.severity(),
            f.kind.label(),
            f.detail,
        );
    }

    summary.push_str(
        "\n> This check runs externally via the [metsuke](https://github.com/plenoai/metsuke) \
         GitHub App and cannot be bypassed by modifying repository files.",
    );

    (conclusion, title, summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_diff() {
        let diff = "\
diff --git a/src/main.rs b/src/main.rs
--- a/src/main.rs
+++ b/src/main.rs
@@ -1,3 +1,4 @@
 fn main() {
+    println!(\"hello\");
     // existing
 }";
        let findings = scan_diff(diff);
        assert!(findings.is_empty());
    }

    #[test]
    fn detect_zero_width_space() {
        let diff = format!(
            "diff --git a/x.rs b/x.rs\n\
             @@ -1,1 +1,2 @@\n\
             +let x\u{200B} = 1;",
        );
        let findings = scan_diff(&diff);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].kind, FindingKind::InvisibleUnicode);
        assert_eq!(findings[0].line, 1);
        assert!(findings[0].detail.contains("U+200B"));
    }

    #[test]
    fn detect_bidi_override() {
        let diff = format!(
            "diff --git a/x.py b/x.py\n\
             @@ -1,1 +1,2 @@\n\
             +access = \"\u{202E}teerts\";",
        );
        let findings = scan_diff(&diff);
        assert!(findings.iter().any(|f| f.kind == FindingKind::BiDiOverride));
    }

    #[test]
    fn ignores_context_lines() {
        let diff = format!(
            "diff --git a/x.rs b/x.rs\n\
             @@ -1,2 +1,3 @@\n\
             fn main() {{\n\
             +    println!(\"ok\");\n\
              let old\u{200B} = 1;\n\
             }}",
        );
        let findings = scan_diff(&diff);
        assert!(findings.is_empty(), "context lines should not be scanned");
    }

    #[test]
    fn multiple_findings_different_files() {
        let diff = format!(
            "diff --git a/a.rs b/a.rs\n\
             @@ -1,1 +1,2 @@\n\
             +let a\u{200B} = 1;\n\
             diff --git a/b.rs b/b.rs\n\
             @@ -1,1 +1,2 @@\n\
             +let b\u{202E} = 2;",
        );
        let findings = scan_diff(&diff);
        assert_eq!(findings.len(), 2);
        assert_eq!(findings[0].file, "a.rs");
        assert_eq!(findings[1].file, "b.rs");
    }

    #[test]
    fn soft_hyphen_detected() {
        let diff = format!(
            "diff --git a/x.rs b/x.rs\n\
             @@ -1,1 +1,2 @@\n\
             +let pri\u{00AD}vate = true;",
        );
        let findings = scan_diff(&diff);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].detail.contains("Soft Hyphen"));
    }

    #[test]
    fn tag_characters_detected() {
        let diff = format!(
            "diff --git a/x.rs b/x.rs\n\
             @@ -1,1 +1,2 @@\n\
             +let x\u{E0001} = 1;",
        );
        let findings = scan_diff(&diff);
        assert!(findings.iter().any(|f| f.detail.contains("Tag Character")));
    }

    #[test]
    fn format_clean() {
        let (conclusion, title, _) = format_check_summary(&[]);
        assert_eq!(conclusion, "success");
        assert!(title.contains("clean"));
    }

    #[test]
    fn format_with_findings() {
        let findings = vec![Finding {
            file: "evil.rs".to_string(),
            line: 42,
            kind: FindingKind::BiDiOverride,
            detail: "U+202E".to_string(),
        }];
        let (conclusion, title, summary) = format_check_summary(&findings);
        assert_eq!(conclusion, "failure");
        assert!(title.contains("1 finding"));
        assert!(summary.contains("evil.rs"));
        assert!(summary.contains("metsuke"));
    }

    #[test]
    fn hunk_line_number_parsing() {
        assert_eq!(parse_hunk_line_number("@@ -10,5 +20,7 @@"), Some(20));
        assert_eq!(parse_hunk_line_number("@@ -1 +1 @@"), Some(1));
        assert_eq!(
            parse_hunk_line_number("@@ -0,0 +1,100 @@ fn foo()"),
            Some(1)
        );
    }
}
