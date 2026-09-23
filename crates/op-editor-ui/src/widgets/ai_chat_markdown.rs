//! CommonMark events adapted to the native transcript's fixed-height rows.
use pulldown_cmark::{Event, Parser, Tag, TagEnd};

use super::{wrap_spans, RichLine, Span, SpanStyle, BULLET_INDENT, CHAR_UNIT_PX};

struct Layout {
    lines: Vec<RichLine>,
    spans: Vec<Span>,
    budget: u32,
    strong: bool,
    emphasis: bool,
    heading: bool,
    code: bool,
    quote_depth: usize,
    lists: Vec<Option<u64>>,
    bullet: bool,
}

impl Layout {
    fn push(&mut self, text: &str, style: SpanStyle) {
        if let Some(last) = self.spans.last_mut().filter(|last| last.style == style) {
            last.text.push_str(text);
        } else if !text.is_empty() {
            self.spans.push(Span {
                text: text.into(),
                style,
            });
        }
    }

    fn style(&self) -> SpanStyle {
        match (self.code, self.heading, self.strong, self.emphasis) {
            (true, _, _, _) => SpanStyle::Code,
            (_, true, _, _) => SpanStyle::Heading,
            (_, _, true, true) => SpanStyle::StrongEmphasis,
            (_, _, true, _) => SpanStyle::Strong,
            (_, _, _, true) => SpanStyle::Emphasis,
            _ => SpanStyle::Body,
        }
    }

    fn flush(&mut self, even_empty: bool) {
        if self.spans.is_empty() && !even_empty {
            return;
        }
        let inset = (self.lists.len() + self.quote_depth) as f32 * BULLET_INDENT;
        let budget = self
            .budget
            .saturating_sub((inset / CHAR_UNIT_PX) as u32)
            .max(8);
        let spans = std::mem::take(&mut self.spans);
        // Code remains exact, including indentation and blank lines. Clipping
        // is owned by the transcript, not by dropping characters at wrap time.
        let wrapped = if self.code {
            vec![spans]
        } else {
            wrap_spans(&spans, budget)
        };
        for (index, spans) in wrapped.into_iter().enumerate() {
            self.lines.push(RichLine {
                spans,
                inset,
                bullet: self.bullet && index == 0,
                code_block_width: if self.code {
                    budget as f32 * CHAR_UNIT_PX
                } else {
                    0.0
                },
                quote: self.quote_depth > 0,
            });
        }
        self.bullet = false;
    }
}

pub(crate) fn layout_rich(text: &str, budget: u32) -> Vec<RichLine> {
    let mut layout = Layout {
        lines: Vec::new(),
        spans: Vec::new(),
        budget,
        strong: false,
        emphasis: false,
        heading: false,
        code: false,
        quote_depth: 0,
        lists: Vec::new(),
        bullet: false,
    };
    // The event stream handles nesting, escapes, variable-length code fences,
    // and incomplete streaming Markdown without our own delimiter guesses.
    for event in Parser::new(text) {
        match event {
            Event::Start(Tag::Strong) => layout.strong = true,
            Event::End(TagEnd::Strong) => layout.strong = false,
            Event::Start(Tag::Emphasis) => layout.emphasis = true,
            Event::End(TagEnd::Emphasis) => layout.emphasis = false,
            Event::Start(Tag::Heading { .. }) => {
                layout.flush(false);
                layout.heading = true;
            }
            Event::End(TagEnd::Heading(_)) => {
                layout.flush(false);
                layout.heading = false;
            }
            Event::Start(Tag::CodeBlock(_)) => {
                layout.flush(false);
                layout.code = true;
            }
            Event::End(TagEnd::CodeBlock) => {
                layout.flush(false);
                layout.code = false;
            }
            Event::Start(Tag::BlockQuote(_)) => {
                layout.flush(false);
                layout.quote_depth += 1;
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                layout.flush(false);
                layout.quote_depth = layout.quote_depth.saturating_sub(1);
            }
            Event::Start(Tag::List(start)) => {
                layout.flush(false);
                layout.lists.push(start);
            }
            Event::End(TagEnd::List(_)) => {
                layout.flush(false);
                layout.lists.pop();
            }
            Event::Start(Tag::Item) => {
                layout.flush(false);
                if let Some(Some(number)) = layout.lists.last_mut() {
                    let label = format!("{number}. ");
                    *number = number.saturating_add(1);
                    layout.push(&label, SpanStyle::Body);
                } else {
                    layout.bullet = true;
                }
            }
            Event::End(TagEnd::Item | TagEnd::Paragraph) => layout.flush(false),
            Event::Text(text) if layout.code => {
                for part in text.split_inclusive('\n') {
                    layout.push(part.strip_suffix('\n').unwrap_or(part), SpanStyle::Code);
                    if part.ends_with('\n') {
                        layout.flush(true);
                    }
                }
            }
            Event::Text(text) => layout.push(&text, layout.style()),
            Event::Code(text) => layout.push(&text, SpanStyle::Code),
            Event::SoftBreak | Event::HardBreak => layout.flush(true),
            // Treat raw HTML as literal text. Never create DOM or fetch media.
            Event::Html(text) | Event::InlineHtml(text) => layout.push(&text, SpanStyle::Body),
            Event::Rule => {
                layout.flush(false);
                layout.push("────────", SpanStyle::Body);
                layout.flush(false);
            }
            // Labels/alt text remain readable. Links are not activated here.
            _ => {}
        }
    }
    layout.flush(false);
    layout.lines
}

#[cfg(test)]
pub(crate) fn parse_spans(text: &str) -> Vec<Span> {
    layout_rich(text, u32::MAX)
        .into_iter()
        .flat_map(|line| line.spans)
        .collect()
}
