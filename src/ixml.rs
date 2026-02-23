use crate::prelude::*;

pub enum MetadataBlock {
    BEXT,
    USER,
    ASWG,
    STEINBERG,
}

impl MetadataBlock {
    pub fn as_str(&self) -> &str {
        match self {
            MetadataBlock::BEXT => "BEXT",
            MetadataBlock::USER => "USER",
            MetadataBlock::ASWG => "ASWG",
            MetadataBlock::STEINBERG => "STEINBERG",
        }
    }
}
impl Metadata {
    pub fn parse_ixml(&mut self, ixml: &str) -> R<()> {
        let mut block: Option<MetadataBlock> = None;
        let mut key: Option<String> = None;
        let mut val: Option<String> = None;

        for line in ixml.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue; // Skip empty lines and comments
            }
            match line {
                "</BEXT>" | "</USER>" | "</ASWG>" | "</STEINBERG>" => block = None,
                "<BEXT>" => {
                    block = Some(MetadataBlock::BEXT);
                    continue;
                }
                "<USER>" => {
                    block = Some(MetadataBlock::USER);
                    continue;
                }
                "<ASWG>" => {
                    block = Some(MetadataBlock::ASWG);
                    continue;
                }
                "<STEINBERG>" => {
                    block = Some(MetadataBlock::STEINBERG);
                    continue;
                }
                _ => {}
            }
            let Some(block) = &block else {
                continue;
            };

            match block {
                MetadataBlock::STEINBERG => {
                    if line.starts_with("<NAME>") {
                        key = Some(line.replace("<NAME>", "").replace("</NAME>", ""));
                    } else if line.starts_with("<VALUE>") {
                        val = Some(line.replace("<VALUE>", "").replace("</VALUE>", ""));
                    }
                }
                _ => {
                    let r: Vec<&str> = line.split(['<', '>']).collect();
                    if r.len() >= 3 {
                        key = Some(r[1].trim().to_string());
                        val = Some(r[2].trim().to_string());
                    }
                }
            }

            if let (Some(k), Some(v)) = (key.take(), val.take()) {
                self.set_field(
                    &format!("{}_{}", block.as_str(), k),
                    &v,
                )?;
            }
        }

        if let Some(description) = self.get_field("BEXT_BWF_DESCRIPTION")
            && description.starts_with("zTAKE")
        {
            description.split("z").for_each(|part| {
                if part.is_empty() {
                    return;
                }
                let (key, val) = part.split_once('=').unwrap_or((part, ""));
                let _ = self.set_field(&format!("USER_{}", key.trim()), val.trim());
            });
        }

        self.set_field("USER_EMBEDDER", "FFCodex")?;
        self.set_field("BEXT_BWF_CODING_HISTORY", "FFCodex")?;

        Ok(())
    }
}

pub fn create_ixml_from_metadata(metadata: &Metadata) -> R<String> {
    let mut bext = String::from("<BEXT>\n");
    let mut steinberg = String::from("<STEINBERG>\n <ATTR_LIST>\n");
    let mut user = String::from("<USER>\n");
    let mut aswg = String::from("<ASWG>\n");

    for (k, v) in metadata.get_all_fields() {
        if let Some(key) = k.strip_prefix("BEXT_") {
            bext.push_str(&format!("  <{}>{}</{}>\n", key, xml_escape(v), key));
        } else if let Some(key) = k.strip_prefix("STEINBERG_") {
            steinberg.push_str(&format!(
                    "  <ATTR>\n    <TYPE>string</TYPE>\n    <NAME>{}</NAME>\n    <VALUE>{}</VALUE>\n  </ATTR>\n",
                    key,
                    xml_escape(v)
                ));
        } else if let Some(key) = k.strip_prefix("USER_") {
            user.push_str(&format!("  <{}>{}</{}>\n", key, xml_escape(v), key));
        } else if let Some(key) = k.strip_prefix("ASWG_") {
            aswg.push_str(&format!("  <{}>{}</{}>\n", key, xml_escape(v), key));
        }
    }

    bext.push_str("</BEXT>\n");
    steinberg.push_str("  </ATTR_LIST>\n</STEINBERG>\n");
    user.push_str("</USER>\n");
    aswg.push_str("</ASWG>\n");

    let mut xml = String::new();
    // xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    // xml.push_str("<BWFXML>\n");
    xml.push_str("  <IXML_VERSION>1.61</IXML_VERSION>\n");
    xml.push_str(&bext);
    xml.push_str(&aswg);
    xml.push_str(&steinberg);
    xml.push_str(&user);

    Ok(xml)
}

pub fn xml_escape(text: &str) -> String {
    // Check if the text is already XML-escaped to avoid double-encoding
    if text.contains("&amp;")
        || text.contains("&lt;")
        || text.contains("&gt;")
        || text.contains("&quot;")
        || text.contains("&apos;")
    {
        // Text appears to already be XML-escaped, return as-is
        text.to_string()
    } else {
        // Apply XML escaping
        text.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&apos;")
    }
}
