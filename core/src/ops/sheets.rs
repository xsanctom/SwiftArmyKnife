//! Spreadsheet conversions: CSV ⇄ XLSX, via python3 + openpyxl.
//!
//! openpyxl (bundled with the Homebrew python3) reads/writes .xlsx and we use
//! the stdlib `csv` module for the CSV side. One small script handles both
//! directions, keyed on the output extension. (Old binary .xls isn't supported
//! — openpyxl can't read it.)

use super::{JobParams, Op, OpId, Stage};
use crate::probe::ProbeResult;
use std::path::Path;

/// The converter, run as `python3 -c SCRIPT <input> <output>`.
const SCRIPT: &str = r#"import sys, csv
inp, out = sys.argv[1], sys.argv[2]
if out.lower().endswith('.xlsx'):
    from openpyxl import Workbook
    wb = Workbook(); ws = wb.active
    with open(inp, newline='', encoding='utf-8-sig') as f:
        for row in csv.reader(f):
            ws.append(row)
    wb.save(out)
else:
    from openpyxl import load_workbook
    wb = load_workbook(inp, read_only=True, data_only=True)
    ws = wb.active
    with open(out, 'w', newline='', encoding='utf-8') as f:
        w = csv.writer(f, lineterminator='\n')
        for row in ws.iter_rows(values_only=True):
            w.writerow(['' if c is None else c for c in row])
"#;

fn stage(input: &str, output: &str) -> Vec<Stage> {
    vec![Stage::python(
        vec!["-c".into(), SCRIPT.into(), input.into(), output.into()],
        1.0,
    )]
}

// MARK: CSV → XLSX

pub struct SheetToXlsx;

impl Op for SheetToXlsx {
    fn id(&self) -> OpId {
        OpId::SheetToXlsx
    }
    fn label(&self) -> &'static str {
        "Convert to XLSX"
    }
    fn output_suffix(&self, _params: &JobParams) -> String {
        String::new()
    }
    fn output_ext(&self, _input: &str, _params: &JobParams) -> String {
        "xlsx".into()
    }
    fn build_stages(
        &self,
        input: &str,
        output: &str,
        _workdir: &Path,
        _probe: &ProbeResult,
        _params: &JobParams,
    ) -> Vec<Stage> {
        stage(input, output)
    }
}

// MARK: XLSX → CSV

pub struct SheetToCsv;

impl Op for SheetToCsv {
    fn id(&self) -> OpId {
        OpId::SheetToCsv
    }
    fn label(&self) -> &'static str {
        "Convert to CSV"
    }
    fn output_suffix(&self, _params: &JobParams) -> String {
        String::new()
    }
    fn output_ext(&self, _input: &str, _params: &JobParams) -> String {
        "csv".into()
    }
    fn build_stages(
        &self,
        input: &str,
        output: &str,
        _workdir: &Path,
        _probe: &ProbeResult,
        _params: &JobParams,
    ) -> Vec<Stage> {
        stage(input, output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::Tool;

    fn p() -> ProbeResult {
        ProbeResult {
            is_video: false,
            is_image: false,
            is_sheet: true,
            duration_s: 0.0,
            width: 0,
            height: 0,
            video_codec: String::new(),
            has_audio: false,
            audio_codec: String::new(),
        }
    }

    #[test]
    fn csv_to_xlsx_runs_python() {
        assert_eq!(
            SheetToXlsx.output_ext("data.csv", &JobParams::default()),
            "xlsx"
        );
        let s = SheetToXlsx.build_stages(
            "data.csv",
            "data.xlsx",
            Path::new("/wd"),
            &p(),
            &JobParams::default(),
        );
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].tool, Tool::Python);
        assert_eq!(s[0].args.first().unwrap(), "-c");
        assert_eq!(s[0].args.last().unwrap(), "data.xlsx");
    }

    #[test]
    fn xlsx_to_csv_runs_python() {
        assert_eq!(
            SheetToCsv.output_ext("book.xlsx", &JobParams::default()),
            "csv"
        );
        let s = SheetToCsv.build_stages(
            "book.xlsx",
            "book.csv",
            Path::new("/wd"),
            &p(),
            &JobParams::default(),
        );
        assert_eq!(s[0].tool, Tool::Python);
        assert_eq!(s[0].args.last().unwrap(), "book.csv");
    }
}
