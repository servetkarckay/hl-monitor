mod overview;
pub mod tape;

use crate::app::AppState;

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

pub fn draw(f: &mut ratatui::Frame, state: &AppState) {
    let area = f.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(area);
    let body = chunks[0];
    let status_area = chunks[1];

    // Single consolidated dashboard
    overview::render(f, body, state);

    // Status bar
    let status = format!(
        " {} | {} | last refresh: {} | {}",
        state.user,
        if state.connection_ok {
            "connected"
        } else {
            "DISCONNECTED"
        },
        state
            .last_refresh
            .map(|i| format!("{:.1}s ago", i.elapsed().as_secs_f64()))
            .unwrap_or_else(|| "—".to_string()),
        state.error.clone().unwrap_or_default()
    );
    let style = if state.connection_ok {
        Style::default().bg(Color::DarkGray).fg(Color::White)
    } else {
        Style::default().bg(Color::Red).fg(Color::White)
    };
    let p = Paragraph::new(status).style(style);
    f.render_widget(p, status_area);

    if state.input_mode {
        let area = centered_rect(60, 20, f.area());
        f.render_widget(Clear, area);
        let text = format!("Filter coin: {}_", state.filter_buffer);
        let p = Paragraph::new(text)
            .block(Block::default().title("Filter").borders(Borders::ALL))
            .wrap(Wrap { trim: true });
        f.render_widget(p, area);
    }
    if state.show_help {
        let area = centered_rect(70, 60, f.area());
        f.render_widget(Clear, area);
        let help = "Keys:\nq quit\nTab/Shift+Tab or Left/Right change tab\n1-7 jump to tab\n/ filter by coin (Enter confirm, Esc cancel)\nc clear filter\ne export fills to CSV\nj/k or Up/Down scroll\nPgUp/PgDn scroll faster, g/Home top\n? toggle this help\n";
        let p = Paragraph::new(help)
            .block(Block::default().title("Help").borders(Borders::ALL))
            .wrap(Wrap { trim: true });
        f.render_widget(p, area);
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
