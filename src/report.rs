use crate::{checker::IllegalExpr, config::Config, test::TestResult};
use anyhow::Result;
use serde::Serialize;
use std::{
    collections::HashMap,
    fmt::Write as FmtWrite,
    path::{Path, PathBuf},
};

#[derive(Serialize)]
pub struct RunReport {
    pub unpack: UnpackSummary,
    pub totals: TotalsSummary,
    pub security: SecuritySummary,
    pub submissions: Vec<SubmissionReport>,
}

#[derive(Serialize)]
pub struct UnpackSummary {
    pub prepared: usize,
    pub skipped: usize,
    pub failed: usize,
}

#[derive(Serialize)]
pub struct TotalsSummary {
    pub submissions: usize,
    pub submissions_with_issues: usize,
    pub perfect_scores: usize,
    pub max_points_per_submission: u64,
    pub cases_total: usize,
    pub cases_passed: usize,
}

#[derive(Serialize)]
pub struct SecuritySummary {
    pub flagged_files: usize,
    pub findings: Vec<SecurityFinding>,
}

#[derive(Serialize)]
pub struct SecurityFinding {
    pub file: String,
    pub issues: Vec<SecurityIssue>,
}

#[derive(Serialize)]
pub struct SecurityIssue {
    pub line: usize,
    pub column: usize,
    pub violation: Option<String>,
    pub snippet: Option<String>,
}

#[derive(Serialize)]
pub struct SubmissionReport {
    pub name: String,
    pub path: String,
    pub points_awarded: u64,
    pub max_points: u64,
    pub cases: Vec<CaseReport>,
}

#[derive(Serialize)]
pub struct CaseReport {
    pub index: usize,
    pub input: String,
    pub expected: String,
    pub points: u64,
    pub outcome: CaseOutcome,
}

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum CaseOutcome {
    Correct { output: String },
    Wrong { output: String, diff: DiffSummary },
    Error { code: i32, reason: String },
}

#[derive(Serialize)]
pub struct DiffSummary {
    pub additions: usize,
    pub removals: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputFormat {
    Json,
    Toml,
    Plaintext,
}

#[derive(Debug, Default)]
pub struct TestTotals {
    pub total_cases: usize,
    pub passed_cases: usize,
    pub submissions_with_issues: usize,
    pub perfect_scores: usize,
}

pub fn summarize_security(results: &HashMap<PathBuf, Vec<IllegalExpr>>) -> SecuritySummary {
    let findings = results
        .iter()
        .map(|(path, issues)| SecurityFinding {
            file: path.display().to_string(),
            issues: issues
                .iter()
                .map(|issue| SecurityIssue {
                    line: issue.loc.1.saturating_add(1),
                    column: issue.loc.0.saturating_add(1),
                    violation: issue
                        .violates
                        .as_ref()
                        .map(|rule| rule.as_ref().to_string()),
                    snippet: issue.content.clone(),
                })
                .collect(),
        })
        .collect::<Vec<_>>();
    SecuritySummary {
        flagged_files: findings.len(),
        findings,
    }
}

pub fn summarize_submissions(
    results: Vec<(PathBuf, Vec<TestResult>)>,
    config: &Config,
    max_points_per_submission: u64,
) -> (Vec<SubmissionReport>, Vec<(String, u64)>, TestTotals) {
    let mut submissions = Vec::new();
    let mut scoreboard = Vec::new();
    let mut totals = TestTotals::default();

    for (path, test_results) in results {
        let mut cases = Vec::new();
        let mut submission_points = 0;
        for (idx, result) in test_results.into_iter().enumerate() {
            totals.total_cases += 1;
            match result {
                TestResult::Correct { case, output } => {
                    totals.passed_cases += 1;
                    submission_points += case.points;
                    cases.push(CaseReport {
                        index: idx,
                        input: case.input.clone(),
                        expected: case.expected.clone(),
                        points: case.points,
                        outcome: CaseOutcome::Correct { output },
                    });
                }
                TestResult::Wrong { case, output, diff } => {
                    cases.push(CaseReport {
                        index: idx,
                        input: case.input.clone(),
                        expected: case.expected.clone(),
                        points: case.points,
                        outcome: CaseOutcome::Wrong {
                            output,
                            diff: DiffSummary {
                                additions: diff.count_additions() as usize,
                                removals: diff.count_removals() as usize,
                            },
                        },
                    });
                }
                TestResult::Error { code, reason } => {
                    let (input, expected, points) = config
                        .testcases
                        .get(idx)
                        .map(|case| (case.input.clone(), case.expected.clone(), case.points))
                        .unwrap_or_else(|| (String::new(), String::new(), 0));
                    cases.push(CaseReport {
                        index: idx,
                        input,
                        expected,
                        points,
                        outcome: CaseOutcome::Error { code, reason },
                    });
                }
            }
        }
        let name = match path.file_name().and_then(|name| name.to_str()) {
            Some(name) => name.to_owned(),
            None => path.display().to_string(),
        };
        scoreboard.push((name.clone(), submission_points));
        submissions.push(SubmissionReport {
            name,
            path: path.display().to_string(),
            points_awarded: submission_points,
            max_points: max_points_per_submission,
            cases,
        });
    }

    totals.perfect_scores = submissions
        .iter()
        .filter(|report| report.points_awarded == max_points_per_submission)
        .count();
    totals.submissions_with_issues = submissions
        .iter()
        .filter(|report| report.points_awarded != max_points_per_submission)
        .count();

    (submissions, scoreboard, totals)
}

pub fn detect_output_format(path: &Path) -> (OutputFormat, bool) {
    let ext = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase());
    match ext.as_deref() {
        Some("json") => (OutputFormat::Json, true),
        Some("toml") => (OutputFormat::Toml, true),
        Some("txt") | None => (OutputFormat::Plaintext, true),
        _ => (OutputFormat::Plaintext, false),
    }
}

pub fn serialize_report(report: &RunReport, format: OutputFormat) -> Result<Vec<u8>> {
    match format {
        OutputFormat::Json => Ok(serde_json::to_vec_pretty(report)?),
        OutputFormat::Toml => Ok(toml::to_string_pretty(report)?.into_bytes()),
        OutputFormat::Plaintext => Ok(render_plain(report).into_bytes()),
    }
}

pub fn render_plain(report: &RunReport) -> String {
    let mut buf = String::new();
    let _ = writeln!(
        &mut buf,
        "Unpack summary: prepared={}, skipped={}, failed={}",
        report.unpack.prepared, report.unpack.skipped, report.unpack.failed
    );
    let _ = writeln!(
        &mut buf,
        "Totals: submissions={}, with_issues={}, perfect={}, cases_passed={}/{} (max_points={})",
        report.totals.submissions,
        report.totals.submissions_with_issues,
        report.totals.perfect_scores,
        report.totals.cases_passed,
        report.totals.cases_total,
        report.totals.max_points_per_submission
    );
    if !report.security.findings.is_empty() {
        let _ = writeln!(
            &mut buf,
            "Security: {} flagged file(s).",
            report.security.flagged_files
        );
        for finding in &report.security.findings {
            let _ = writeln!(&mut buf, "  - {}", finding.file);
            for issue in &finding.issues {
                let _ = writeln!(
                    &mut buf,
                    "      line {}, column {}: violation {:?}, snippet {:?}",
                    issue.line, issue.column, issue.violation, issue.snippet
                );
            }
        }
    }
    for submission in &report.submissions {
        let _ = writeln!(
            &mut buf,
            "\nSubmission: {} (path: {}) => {}/{}",
            submission.name, submission.path, submission.points_awarded, submission.max_points
        );
        for case in &submission.cases {
            match &case.outcome {
                CaseOutcome::Correct { output } => {
                    let _ = writeln!(
                        &mut buf,
                        "  - case {} correct (+{} pts)",
                        case.index, case.points
                    );
                    if !output.is_empty() {
                        let _ = writeln!(&mut buf, "      output: {:?}", output);
                    }
                }
                CaseOutcome::Wrong { output, diff } => {
                    let _ = writeln!(
                        &mut buf,
                        "  - case {} wrong (+0/{})",
                        case.index, case.points
                    );
                    let _ = writeln!(&mut buf, "      expected: {:?}", case.expected);
                    let _ = writeln!(&mut buf, "      got: {:?}", output);
                    let _ = writeln!(
                        &mut buf,
                        "      diff summary: +{} additions, -{} removals",
                        diff.additions, diff.removals
                    );
                }
                CaseOutcome::Error { code, reason } => {
                    let _ = writeln!(
                        &mut buf,
                        "  - case {} error code {} ({})",
                        case.index, code, reason
                    );
                }
            }
            if !case.input.is_empty() {
                let _ = writeln!(&mut buf, "      input: {:?}", case.input);
            }
        }
    }
    buf
}
