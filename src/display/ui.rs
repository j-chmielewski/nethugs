use std::{
    collections::{HashMap, VecDeque},
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
    ) {
        self.state.update(connections_to_procs, utilization);
    }
    pub fn end(&mut self) {
        self.terminal.show_cursor().unwrap();
    }
}

const HEADER_HEIGHT: u16 = 1;
const ROW_HEIGHT: u16 = 1;
const COLUMN_GAP: u16 = 1;
const CHART_COLOR_START: Color = Color::Rgb(0, 195, 255);
const CHART_COLOR_END: Color = Color::Rgb(170, 70, 255);

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

    let (max_download, max_upload) = max_history_values(state);

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
        render_process_row(
            frame,
            row_rect,
            row,
            state.unit_family,
            max_download,
            max_upload,
        );
    }
}

fn render_table_header(frame: &mut Frame, rect: Rect) {
    let columns = split_columns(rect);
    let headers = [
        "Process",
        "Down",
        "Up",
        "Total Down",
        "Total Up",
        "Down",
        "Up",
    ];

    for (col, title) in columns.into_iter().zip(headers) {
        let header = Paragraph::new(Span::styled(
            title,
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ))
        .alignment(Alignment::Center);
        frame.render_widget(header, col);
    }
}

fn render_process_row(
    frame: &mut Frame,
    rect: Rect,
    row: &crate::display::ProcessRow,
    unit_family: crate::display::BandwidthUnitFamily,
    max_download: f64,
    max_upload: f64,
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

    render_bar_chart(
        frame,
        columns[5],
        &row.download_history,
        max_download,
        Color::Cyan,
    );
    render_bar_chart(
        frame,
        columns[6],
        &row.upload_history,
        max_upload,
        Color::Magenta,
    );
}

fn render_bar_chart(
    frame: &mut Frame,
    rect: Rect,
    history: &VecDeque<f64>,
    global_max: f64,
    color: Color,
) {
    if rect.width == 0 || rect.height == 0 {
        return;
    }

    let (bars, max_value) = history_to_bars(history, rect.width as usize, global_max);
    if bars.is_empty() {
        return;
    }

    let _ = color;
    let group = BarGroup::default().bars(&bars);
    let chart = BarChart::default()
        .bar_width(1)
        .bar_gap(0)
        .group_gap(0)
        .bar_style(Style::default().fg(CHART_COLOR_START))
        .max(max_value)
        .data(group);

    frame.render_widget(chart, rect);
}

fn history_to_bars(
    history: &VecDeque<f64>,
    target_len: usize,
    global_max: f64,
) -> (Vec<Bar<'static>>, u64) {
    const CHART_HEADROOM: f64 = 1.1;
    const CHART_MAX_TICKS: u64 = 8;

    if history.is_empty() || target_len == 0 {
        return (Vec::new(), CHART_MAX_TICKS);
    }

    let mut max_value = 0.0_f64;
    let values = fixed_history_window(history, target_len)
        .into_iter()
        .map(|value| {
            let value = if value > u64::MAX as f64 {
                u64::MAX as f64
            } else {
                value
            };
            if value > max_value {
                max_value = value;
            }
            value
        })
        .collect::<Vec<_>>();

    let scale_basis = if global_max > 0.0 {
        global_max
    } else {
        max_value
    };
    let scale_max = if scale_basis <= 0.0 {
        1.0
    } else {
        scale_basis * CHART_HEADROOM
    };

    let bars = values
        .into_iter()
        .map(|value| {
            let ratio = (value / scale_max).clamp(0.0, 1.0);
            let ticks =
                ((ratio * (CHART_MAX_TICKS as f64 - 1.0)).ceil() as u64).clamp(1, CHART_MAX_TICKS);
            let color_ratio = (ticks.saturating_sub(1)) as f64 / (CHART_MAX_TICKS - 1) as f64;
            let color = gradient_color(color_ratio);
            Bar::default()
                .value(ticks)
                .text_value(String::new())
                .style(Style::default().fg(color))
        })
        .collect();

    (bars, CHART_MAX_TICKS)
}

fn max_history_values(state: &UIState) -> (f64, f64) {
    let mut max_download = 0.0_f64;
    let mut max_upload = 0.0_f64;
    for row in &state.process_rows {
        for value in &row.download_history {
            if *value > max_download {
                max_download = *value;
            }
        }
        for value in &row.upload_history {
            if *value > max_upload {
                max_upload = *value;
            }
        }
    }
    (max_download, max_upload)
}

fn gradient_color(ratio: f64) -> Color {
    let ratio = ratio.clamp(0.0, 1.0);
    let (sr, sg, sb) = color_to_rgb(CHART_COLOR_START);
    let (er, eg, eb) = color_to_rgb(CHART_COLOR_END);
    let r = sr as f64 + (er as f64 - sr as f64) * ratio;
    let g = sg as f64 + (eg as f64 - sg as f64) * ratio;
    let b = sb as f64 + (eb as f64 - sb as f64) * ratio;
    Color::Rgb(r.round() as u8, g.round() as u8, b.round() as u8)
}

fn color_to_rgb(color: Color) -> (u8, u8, u8) {
    match color {
        Color::Rgb(r, g, b) => (r, g, b),
        _ => (0, 0, 0),
    }
}

fn fixed_history_window(history: &VecDeque<f64>, target_len: usize) -> Vec<f64> {
    if target_len == 0 {
        return Vec::new();
    }
    let history_len = history.len();
    if history_len >= target_len {
        return history
            .iter()
            .skip(history_len - target_len)
            .copied()
            .collect();
    }

    let mut out = Vec::with_capacity(target_len);
    out.extend(std::iter::repeat(0.0).take(target_len - history_len));
    out.extend(history.iter().copied());
    out
}

fn split_columns(rect: Rect) -> Vec<Rect> {
    let constraints = [
        Constraint::Length(24),
        Constraint::Length(COLUMN_GAP),
        Constraint::Length(12),
        Constraint::Length(COLUMN_GAP),
        Constraint::Length(12),
        Constraint::Length(COLUMN_GAP),
        Constraint::Length(12),
        Constraint::Length(COLUMN_GAP),
        Constraint::Length(12),
        Constraint::Length(COLUMN_GAP),
        Constraint::Min(10),
        Constraint::Length(COLUMN_GAP),
        Constraint::Min(10),
    ];

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(constraints)
        .split(rect);

    chunks.iter().step_by(2).copied().collect()
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
