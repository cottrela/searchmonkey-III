use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;
use std::fs;
use std::path::Path;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

const SM_META_SCHEMA: &str = "sm.meta.v1";

#[derive(Debug, Clone, Deserialize)]
pub struct SmMeta {
    pub schema: String,
    pub source: SmSource,
    pub generator: SmGenerator,
    pub text: SmText,
    #[serde(default)]
    pub ranges: Vec<SmRange>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SmSource {
    pub path: String,
    pub size: u64,
    pub mtime: String,
    pub hash: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SmGenerator {
    pub plugin_id: String,
    pub plugin_version: String,
    pub settings_hash: Option<String>,
    pub engine: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SmText {
    pub path: String,
    pub encoding: String,
    pub length_bytes: Option<u64>,
    pub mtime: Option<String>,
    pub hash: Option<String>,
    pub offsets: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SmRangeType {
    Document,
    Page,
    Section,
    Heading,
    Paragraph,
    Block,
    PageBreak,
    ListItem,
    Table,
    Row,
    Cell,
    Footnote,
    Annotation,
    ImageAlt,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SmRange {
    #[serde(rename = "type")]
    pub kind: SmRangeType,
    pub start: u64,
    pub end: u64,
    pub index: Option<u64>,
    pub page: Option<u64>,
    pub level: Option<u64>,
    pub label: Option<String>,
    #[serde(default)]
    pub hints: Vec<String>,
    pub confidence: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct RangeContext {
    pub page: Option<SmRange>,
    pub smallest: SmRange,
}

impl SmMeta {
    pub fn parse_str(contents: &str) -> Result<Self> {
        let meta: Self = serde_json::from_str(contents).context("failed to parse .sm.meta json")?;
        meta.validate()?;
        Ok(meta)
    }

    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let contents = fs::read_to_string(path)
            .with_context(|| format!("failed reading .sm.meta file at {}", path.display()))?;
        Self::parse_str(&contents)
    }

    pub fn validate(&self) -> Result<()> {
        if self.schema != SM_META_SCHEMA {
            bail!(
                "unsupported .sm.meta schema {:?}; expected {}",
                self.schema,
                SM_META_SCHEMA
            );
        }

        if self.source.path.trim().is_empty() {
            bail!("source.path must not be empty");
        }
        parse_rfc3339(&self.source.mtime).context("source.mtime must be RFC 3339")?;
        validate_optional_sha256(&self.source.hash, "source.hash")?;

        if self.generator.plugin_id.trim().is_empty() {
            bail!("generator.plugin_id must not be empty");
        }
        if self.generator.plugin_version.trim().is_empty() {
            bail!("generator.plugin_version must not be empty");
        }
        validate_optional_sha256(&self.generator.settings_hash, "generator.settings_hash")?;

        if self.text.path.trim().is_empty() {
            bail!("text.path must not be empty");
        }
        if self.text.encoding != "utf-8" {
            bail!("text.encoding must be utf-8");
        }
        if let Some(offsets) = &self.text.offsets {
            if offsets != "utf8-bytes" {
                bail!("text.offsets must be utf8-bytes when present");
            }
        }
        if let Some(mtime) = &self.text.mtime {
            parse_rfc3339(mtime).context("text.mtime must be RFC 3339")?;
        }
        validate_optional_sha256(&self.text.hash, "text.hash")?;

        for (index, range) in self.ranges.iter().enumerate() {
            if range.start >= range.end {
                bail!("range {index} must satisfy start < end");
            }
            if let Some(page) = range.page {
                if page == 0 {
                    bail!("range {index} page must be >= 1");
                }
            }
            if let Some(level) = range.level {
                if level == 0 {
                    bail!("range {index} level must be >= 1");
                }
            }
            if let Some(confidence) = range.confidence {
                if !(0.0..=1.0).contains(&confidence) {
                    bail!("range {index} confidence must be between 0 and 1");
                }
            }
        }

        Ok(())
    }

    pub fn context_for_offset(&self, offset: u64) -> Option<RangeContext> {
        let smallest = self
            .ranges
            .iter()
            .filter(|range| range.start <= offset && offset < range.end)
            .min_by_key(|range| (range.end - range.start, range_priority(&range.kind)))?
            .clone();

        let page = self
            .ranges
            .iter()
            .filter(|range| range.kind == SmRangeType::Page)
            .filter(|range| range.start <= offset && offset < range.end)
            .min_by_key(|range| range.end - range.start)
            .cloned();

        Some(RangeContext { page, smallest })
    }
}

fn range_priority(kind: &SmRangeType) -> u8 {
    match kind {
        SmRangeType::Block => 0,
        SmRangeType::PageBreak => 1,
        SmRangeType::ListItem => 2,
        SmRangeType::Heading => 3,
        SmRangeType::Paragraph => 4,
        SmRangeType::Section => 5,
        SmRangeType::Cell => 6,
        SmRangeType::Row => 7,
        SmRangeType::Table => 8,
        SmRangeType::Footnote => 9,
        SmRangeType::Annotation => 10,
        SmRangeType::ImageAlt => 11,
        SmRangeType::Page => 12,
        SmRangeType::Document => 13,
    }
}

fn validate_optional_sha256(value: &Option<String>, field: &str) -> Result<()> {
    let Some(value) = value else {
        return Ok(());
    };

    let Some(hash) = value.strip_prefix("sha256:") else {
        bail!("{field} must start with sha256:");
    };
    if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("{field} must contain 64 hex characters");
    }
    Ok(())
}

fn parse_rfc3339(value: &str) -> Result<OffsetDateTime> {
    OffsetDateTime::parse(value, &Rfc3339).map_err(|err| anyhow!(err))
}

#[cfg(test)]
mod tests {
    use super::{SmMeta, SmRangeType};

    #[test]
    fn parses_valid_meta_and_picks_smallest_range() {
        let meta = SmMeta::parse_str(
            r#"{
              "schema": "sm.meta.v1",
              "source": {
                "path": "/tmp/report.pdf",
                "size": 42,
                "mtime": "2026-05-10T11:22:33Z"
              },
              "generator": {
                "plugin_id": "sm.plugin.pdf",
                "plugin_version": "1.2.3"
              },
              "text": {
                "path": "/tmp/report.pdf.sm.txt",
                "encoding": "utf-8",
                "offsets": "utf8-bytes"
              },
              "ranges": [
                { "type": "page", "start": 0, "end": 200, "page": 7 },
                { "type": "paragraph", "start": 20, "end": 80 },
                { "type": "block", "start": 30, "end": 40, "index": 3 }
              ]
            }"#,
        )
        .unwrap();

        let context = meta.context_for_offset(35).unwrap();
        assert_eq!(context.page.unwrap().page, Some(7));
        assert_eq!(context.smallest.kind, SmRangeType::Block);
        assert_eq!(context.smallest.index, Some(3));
    }

    #[test]
    fn rejects_invalid_ranges() {
        let error = SmMeta::parse_str(
            r#"{
              "schema": "sm.meta.v1",
              "source": {
                "path": "/tmp/report.pdf",
                "size": 42,
                "mtime": "2026-05-10T11:22:33Z"
              },
              "generator": {
                "plugin_id": "sm.plugin.pdf",
                "plugin_version": "1.2.3"
              },
              "text": {
                "path": "/tmp/report.pdf.sm.txt",
                "encoding": "utf-8"
              },
              "ranges": [
                { "type": "page", "start": 40, "end": 40, "page": 1 }
              ]
            }"#,
        )
        .unwrap_err();

        assert!(error.to_string().contains("start < end"));
    }
}
