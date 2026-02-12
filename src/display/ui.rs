use std::{
    collections::{HashMap, VecDeque},
    net::IpAddr,
    time::Duration,
};

use chrono::prelude::*;
use ratatui::{
    backend::Backend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::Span,
    widgets::{Bar, BarChart, BarGroup, Paragraph},
    Frame, Terminal,
};
use unicode_width::UnicodeWidthChar;

use crate::{
    cli::Opt,
    display::{components::HeaderDetails, DisplayBandwidth, UIState},
    network::{LocalSocket, Utilization},
    os::ProcessInfo,
};

pub struct Ui<B>
where
    B: Backend,
{
    terminal: Terminal<B>,
    state: UIState,
}

impl<B> Ui<B>
where
    B: Backend,
{
    pub fn new(terminal_backend: B, opts: &Opt) -> Self {
        let mut terminal = Terminal::new(terminal_backend).unwrap();
        terminal.clear().unwrap();
        terminal.hide_cursor().unwrap();
        let state = {
            let mut state = UIState::default();
            state.interface_name.clone_from(&opts.interface);
            state.unit_family = opts.render_opts.unit_family.into();
            state
        };
        Ui { terminal, state }
    }
    pub fn output_text(&mut self, write_to_stdout: &mut (dyn FnMut(&str) + Send)) {
        let state = &self.state;
        let local_time: DateTime<Local> = Local::now();
        let timestamp = local_time.timestamp();
        let mut no_traffic = true;

        let output_process_data = |write_to_stdout: &mut (dyn FnMut(&str) + Send),
                                   no_traffic: &mut bool| {
            for row in &state.process_rows {
                write_to_stdout(&format!(
                    "process: <{timestamp}> \"{}\" down/up Bps: {}/{} total down/up B: {}/{}",
                    row.process.name,
                    row.current_bytes_downloaded,
                    row.current_bytes_uploaded,
                    row.total_bytes_downloaded,
                    row.total_bytes_uploaded
                ));
                *no_traffic = false;
            }
        };

        // header
        write_to_stdout("Refreshing:");

        output_process_data(write_to_stdout, &mut no_traffic);

        // body2: In case no traffic is detected
        if no_traffic {
            write_to_stdout("<NO TRAFFIC>");
        }

        // footer
        write_to_stdout("");
    }

    pub fn draw(&mut self, paused: bool, elapsed_time: Duration, _table_cycle_offset: usize) {
        self.terminal
            .draw(|frame| {
                let area = frame.area();
                let layout = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(1),
                        Constraint::Min(1),
                        Constraint::Length(1),
                    ])
                    .split(area);

                let header = HeaderDetails {
                    state: &self.state,
                    elapsed_time,
                    paused,
                };
                header.render(frame, layout[0]);

                render_process_table(frame, layout[1], &self.state);
                render_footer(frame, layout[2], paused);
            })
            .unwrap();
    }

    pub fn get_table_count(&self) -> usize {
        1
    }

    pub fn update_state(
        &mut self,
        connections_to_procs: HashMap<LocalSocket, ProcessInfo>,
        utilization: Utilization,
        ip_to_host: HashMap<IpAddr, String>,
    ) {
        self.state.update(connections_to_procs, utilization);
        let _ = ip_to_host;
    }
    pub fn end(&mut self) {
        self.terminal.show_cursor().unwrap();
    }
}

const HEADER_HEIGHT: u16 = 1;
const ROW_HEIGHT: u16 = 1;

fn render_process_table(frame: &mut Frame, rect: Rect, state: &UIState) {
    if rect.height < HEADER_HEIGHT + 1 {
        return;
    }

    let header_rect = Rect {
        x: rect.x,
        y: rect.y,
        width: rect.width,
        height: HEADER_HEIGHT,
    };
    render_table_header(frame, header_rect);

    let body_rect = Rect {
        x: rect.x,
        y: rect.y + HEADER_HEIGHT,
        width: rect.width,
        height: rect.height.saturating_sub(HEADER_HEIGHT),
    };

    let row_slots = body_rect.height / ROW_HEIGHT;
    if row_slots == 0 {
        return;
    }

    if state.process_rows.is_empty() {
        let empty = Paragraph::new("No traffic yet")
            .style(Style::default().fg(Color::Gray))
            .alignment(Alignment::Center);
        frame.render_widget(empty, body_rect);
        return;
    }

    for (index, row) in state
        .process_rows
        .iter()
        .take(row_slots as usize)
        .enumerate()
    {
        let row_rect = Rect {
            x: body_rect.x,
            y: body_rect.y + (index as u16 * ROW_HEIGHT),
            width: body_rect.width,
            height: ROW_HEIGHT,
        };
        render_process_row(frame, row_rect, row, state.unit_family);
    }
}

fn render_table_header(frame: &mut Frame, rect: Rect) {
    let columns = split_columns(rect);
    let headers = [
        "Process",
        "Down/s",
        "Up/s",
        "Total Down",
        "Total Up",
        "Down Chart",
        "Up Chart",
    ];

    for (col, title) in columns.into_iter().zip(headers) {
        let header = Paragraph::new(Span::styled(
            title,
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ));
        frame.render_widget(header, col);
    }
}

fn render_process_row(
    frame: &mut Frame,
    rect: Rect,
    row: &crate::display::ProcessRow,
    unit_family: crate::display::BandwidthUnitFamily,
) {
    let columns = split_columns(rect);
    let name = truncate_to_width(&row.process.name, columns[0].width);
    let down_rate = format!(
        "{}/s",
        DisplayBandwidth {
            bandwidth: row.current_bytes_downloaded as f64,
            unit_family,
        }
    );
    let up_rate = format!(
        "{}/s",
        DisplayBandwidth {
            bandwidth: row.current_bytes_uploaded as f64,
            unit_family,
        }
    );
    let total_down = format!(
        "{}",
        DisplayBandwidth {
            bandwidth: row.total_bytes_downloaded as f64,
            unit_family,
        }
    );
    let total_up = format!(
        "{}",
        DisplayBandwidth {
            bandwidth: row.total_bytes_uploaded as f64,
            unit_family,
        }
    );

    frame.render_widget(Paragraph::new(name), columns[0]);
    frame.render_widget(
        Paragraph::new(down_rate).alignment(Alignment::Right),
        columns[1],
    );
    frame.render_widget(
        Paragraph::new(up_rate).alignment(Alignment::Right),
        columns[2],
    );
    frame.render_widget(
        Paragraph::new(total_down).alignment(Alignment::Right),
        columns[3],
    );
    frame.render_widget(
        Paragraph::new(total_up).alignment(Alignment::Right),
        columns[4],
    );

    render_bar_chart(frame, columns[5], &row.download_history, Color::Cyan);
    render_bar_chart(frame, columns[6], &row.upload_history, Color::Magenta);
}

fn render_bar_chart(frame: &mut Frame, rect: Rect, history: &VecDeque<f64>, color: Color) {
    if rect.width == 0 || rect.height == 0 {
        return;
    }

    let (bars, max_value) = history_to_bars(history);
    if bars.is_empty() {
        return;
    }

    let group = BarGroup::default().bars(&bars);
    let chart = BarChart::default()
        .bar_width(1)
        .bar_gap(0)
        .group_gap(0)
        .bar_style(Style::default().fg(color))
        .max(max_value)
        .data(group);

    frame.render_widget(chart, rect);
}

fn history_to_bars(history: &VecDeque<f64>) -> (Vec<Bar<'static>>, u64) {
    const CHART_HEADROOM: f64 = 1.1;
    const CHART_MAX_TICKS: u64 = 8;

    if history.is_empty() {
        return (Vec::new(), CHART_MAX_TICKS);
    }

    let mut max_value = 0.0_f64;
    let values = history
        .iter()
        .map(|value| {
            let value = if *value > u64::MAX as f64 {
                u64::MAX as f64
            } else {
                *value
            };
            if value > max_value {
                max_value = value;
            }
            value
        })
        .collect::<Vec<_>>();

    let scale_max = if max_value <= 0.0 {
        1.0
    } else {
        max_value * CHART_HEADROOM
    };

    let bars = values
        .into_iter()
        .map(|value| {
            let ratio = (value / scale_max).clamp(0.0, 1.0);
            let ticks =
                ((ratio * (CHART_MAX_TICKS as f64 - 1.0)).ceil() as u64).clamp(1, CHART_MAX_TICKS);
            Bar::default().value(ticks).text_value(String::new())
        })
        .collect();

    (bars, CHART_MAX_TICKS)
}

fn split_columns(rect: Rect) -> Vec<Rect> {
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(24),
            Constraint::Length(12),
            Constraint::Length(12),
            Constraint::Length(12),
            Constraint::Length(12),
            Constraint::Min(10),
            Constraint::Min(10),
        ])
        .split(rect)
        .to_vec()
}

fn truncate_to_width(text: &str, max_width: u16) -> String {
    if max_width == 0 {
        return String::new();
    }
    let mut width = 0;
    let mut out = String::new();
    for ch in text.chars() {
        let ch_width = ch.width().unwrap_or(0) as u16;
        if width + ch_width > max_width {
            break;
        }
        width += ch_width;
        out.push(ch);
    }
    out
}

fn render_footer(frame: &mut Frame, rect: Rect, paused: bool) {
    let status = if paused { "Paused" } else { "Live" };
    let content = format!("{status} | Press <SPACE> to toggle | Press <Q> to quit");
    let footer = Paragraph::new(content)
        .style(
            Style::default()
                .fg(Color::Gray)
                .add_modifier(Modifier::BOLD),
        )
        .alignment(Alignment::Left);
    frame.render_widget(footer, rect);
}
