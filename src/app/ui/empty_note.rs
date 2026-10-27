use ratatui::{
    Frame,
    layout::Rect,
    text::Text,
    widgets::{Block, Borders, Paragraph},
};

use super::Styles;

const ART: &str = include_str!("../../../assets/myna_journal.txt");

pub(super) fn render(frame: &mut Frame, area: Rect, styles: &Styles, active: bool) {
    let style = if active {
        styles.editor.block_normal_active
    } else {
        styles.editor.block_normal_inactive
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(if active {
            "Content - NORMAL"
        } else {
            "Content"
        })
        .style(style);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let art = Text::raw(ART);
    if art.width() > usize::from(inner.width) || art.height() > usize::from(inner.height) {
        return;
    }
    let width = art.width() as u16;
    let height = art.height() as u16;
    let centered = Rect::new(
        inner.x + (inner.width - width) / 2,
        inner.y + (inner.height - height) / 2,
        width,
        height,
    );
    frame.render_widget(Paragraph::new(art).style(style), centered);
}
