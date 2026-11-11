//! Optional egui front end: a human plays the othello engine.
//!
//! Run with `cargo run --release --manifest-path gui/Cargo.toml`.

use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Pos2, Rect, RichText, Sense, Stroke, Vec2,
};
use othello::board::{Board, is_terminal, legal_moves, pass, play, squares};
use othello::search::{SearchConfig, SearchLimits, SearchResult, search_tt};
use othello::tt::{MAX_ENTRIES, Table};
use othello_gui::overlays::{NO_MOVE, frontier, opponent_move_counts};

const FELT: Color32 = Color32::from_rgb(30, 110, 72);
const FELT_DARK: Color32 = Color32::from_rgb(22, 84, 55);
const FRAME: Color32 = Color32::from_rgb(92, 58, 34);
const FRAME_EDGE: Color32 = Color32::from_rgb(64, 40, 24);
const INK: Color32 = Color32::from_rgb(232, 224, 205);
const FLIP_TIME: f32 = 0.28;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Othello")
            .with_inner_size([960.0, 700.0])
            .with_min_inner_size([720.0, 520.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Othello",
        options,
        Box::new(|cc| {
            cc.egui_ctx.set_theme(egui::Theme::Dark);
            Ok(Box::new(App::new()))
        }),
    )
}

/// A position plus the absolute side to move (`Board` itself is side-relative).
#[derive(Clone, Copy)]
struct Position {
    board: Board,
    black_to_move: bool,
}

impl Position {
    const START: Self = Self {
        board: Board::START,
        black_to_move: true,
    };

    fn black_white(self) -> (u64, u64) {
        if self.black_to_move {
            (self.board.me, self.board.opp)
        } else {
            (self.board.opp, self.board.me)
        }
    }

    fn moves(self) -> u64 {
        legal_moves(self.board.me, self.board.opp)
    }
}

struct Thinking {
    started: Instant,
    result: Receiver<(u64, SearchResult)>,
}

struct Flip {
    squares: u64,
    started: Instant,
}

struct App {
    position: Position,
    human_black: bool,
    history: Vec<(Position, Option<u8>)>,
    last_move: Option<u8>,
    think_seconds: f32,
    threshold: u32,
    table: Arc<Mutex<Table>>,
    generation: u64,
    thinking: Option<Thinking>,
    last_search: Option<(SearchResult, Duration, bool)>,
    flip: Option<Flip>,
    show_move_counts: bool,
    show_frontier: bool,
    message: String,
}

impl App {
    fn new() -> Self {
        Self {
            position: Position::START,
            human_black: true,
            history: Vec::new(),
            last_move: None,
            think_seconds: 3.0,
            threshold: SearchConfig::standard().endgame_threshold,
            table: Arc::new(Mutex::new(Table::new(MAX_ENTRIES))),
            generation: 0,
            thinking: None,
            last_search: None,
            flip: None,
            show_move_counts: false,
            show_frontier: false,
            message: "Your move — you play Black.".into(),
        }
    }

    fn new_game(&mut self, human_black: bool) {
        self.generation += 1;
        self.position = Position::START;
        self.human_black = human_black;
        self.history.clear();
        self.last_move = None;
        self.thinking = None;
        self.last_search = None;
        self.flip = None;
        self.table.lock().unwrap().clear();
        self.message = if human_black {
            "Your move — you play Black.".into()
        } else {
            "You play White. The engine opens.".into()
        };
    }

    fn human_to_move(&self) -> bool {
        self.position.black_to_move == self.human_black
    }

    fn apply(&mut self, square: u8) {
        let before = self.position;
        self.history.push((before, self.last_move));
        let after = play(before.board, square);
        self.flip = Some(Flip {
            squares: before.board.opp & after.opp,
            started: Instant::now(),
        });
        self.position = Position {
            board: after,
            black_to_move: !before.black_to_move,
        };
        self.last_move = Some(square);
    }

    fn undo(&mut self) {
        // Step back to the human's previous turn, skipping engine moves and passes.
        while let Some((position, last)) = self.history.pop() {
            self.position = position;
            self.last_move = last;
            if self.human_to_move() && position.moves() != 0 {
                break;
            }
        }
        self.generation += 1;
        self.thinking = None;
        self.flip = None;
        self.message = "Move taken back.".into();
    }

    fn start_engine(&mut self) {
        let (sender, result) = channel();
        let generation = self.generation;
        let board = self.position.board;
        let table = Arc::clone(&self.table);
        let config = SearchConfig {
            endgame_threshold: self.threshold,
            ..SearchConfig::standard()
        };
        let limits = SearchLimits::new(64, (self.think_seconds * 1000.0) as u64, 0);
        thread::spawn(move || {
            let mut table = table.lock().unwrap();
            let found = search_tt(board, limits, &config, &mut table);
            let _ = sender.send((generation, found));
        });
        self.thinking = Some(Thinking {
            started: Instant::now(),
            result,
        });
    }

    /// Advances passes, the engine, and game-over messages; returns true while busy.
    fn step(&mut self) -> bool {
        if let Some(thinking) = &self.thinking {
            match thinking.result.try_recv() {
                Ok((generation, found)) if generation == self.generation => {
                    let elapsed = thinking.started.elapsed();
                    self.thinking = None;
                    let engine_black = self.position.black_to_move;
                    if self.position.moves() & (1u64 << found.best) != 0 {
                        self.last_search = Some((found, elapsed, engine_black));
                        self.apply(found.best);
                        self.message = format!("Engine played {}.", name(found.best));
                    }
                }
                Ok(_) => self.thinking = None,
                Err(_) => return true,
            }
        }
        if is_terminal(self.position.board) {
            let (black, white) = self.position.black_white();
            let (b, w) = (black.count_ones(), white.count_ones());
            let (you, them) = if self.human_black { (b, w) } else { (w, b) };
            self.message = match you.cmp(&them) {
                std::cmp::Ordering::Greater => format!("You win {you}–{them}!"),
                std::cmp::Ordering::Less => format!("Engine wins {them}–{you}."),
                std::cmp::Ordering::Equal => format!("Draw, {you}–{them}."),
            };
            return false;
        }
        if self.position.moves() == 0 {
            let who = if self.human_to_move() {
                "You have"
            } else {
                "The engine has"
            };
            self.message = format!("{who} no legal move — pass.");
            self.history.push((self.position, self.last_move));
            self.position = Position {
                board: pass(self.position.board),
                black_to_move: !self.position.black_to_move,
            };
            return true;
        }
        if !self.human_to_move() && self.flip_progress().is_none() {
            self.start_engine();
            return true;
        }
        false
    }

    fn flip_progress(&self) -> Option<f32> {
        let flip = self.flip.as_ref()?;
        let t = flip.started.elapsed().as_secs_f32() / FLIP_TIME;
        (t < 1.0).then_some(t)
    }

    fn board_ui(&mut self, ui: &mut egui::Ui) {
        let side = ui.available_width().min(ui.available_height()) - 8.0;
        let (response, painter) = ui.allocate_painter(Vec2::splat(side), Sense::click());
        let outer = Rect::from_center_size(response.rect.center(), Vec2::splat(side));
        let margin = side * 0.06;
        let inner = outer.shrink(margin);
        let cell = inner.width() / 8.0;

        // Frame with a soft drop shadow.
        painter.rect_filled(
            outer.translate(Vec2::splat(6.0)),
            CornerRadius::same(18),
            Color32::from_black_alpha(90),
        );
        painter.rect_filled(outer, CornerRadius::same(18), FRAME);
        painter.rect_stroke(
            outer,
            CornerRadius::same(18),
            Stroke::new(2.0, FRAME_EDGE),
            egui::StrokeKind::Inside,
        );
        painter.rect_filled(inner, CornerRadius::same(4), FELT);
        for file in 0..8 {
            for rank in 0..8 {
                if (file + rank) % 2 == 0 {
                    painter.rect_filled(
                        cell_rect(inner, cell, file, rank),
                        CornerRadius::ZERO,
                        FELT_DARK.gamma_multiply(0.35),
                    );
                }
            }
        }
        for i in 0..=8 {
            let offset = i as f32 * cell;
            let stroke = Stroke::new(1.2, Color32::from_black_alpha(110));
            painter.line_segment(
                [
                    inner.left_top() + Vec2::new(offset, 0.0),
                    inner.left_bottom() + Vec2::new(offset, 0.0),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    inner.left_top() + Vec2::new(0.0, offset),
                    inner.right_top() + Vec2::new(0.0, offset),
                ],
                stroke,
            );
        }
        for (x, y) in [(2.0, 2.0), (6.0, 2.0), (2.0, 6.0), (6.0, 6.0)] {
            painter.circle_filled(
                inner.left_top() + Vec2::new(x, y) * cell,
                cell * 0.06,
                Color32::from_black_alpha(140),
            );
        }
        let label = FontId::proportional(margin * 0.42);
        for i in 0..8 {
            let x = inner.left() + (i as f32 + 0.5) * cell;
            let y = inner.top() + (i as f32 + 0.5) * cell;
            let file = ((b'a' + i as u8) as char).to_string();
            let rank = (8 - i).to_string();
            painter.text(
                Pos2::new(x, outer.top() + margin * 0.5),
                Align2::CENTER_CENTER,
                &file,
                label.clone(),
                INK,
            );
            painter.text(
                Pos2::new(x, outer.bottom() - margin * 0.5),
                Align2::CENTER_CENTER,
                &file,
                label.clone(),
                INK,
            );
            painter.text(
                Pos2::new(outer.left() + margin * 0.5, y),
                Align2::CENTER_CENTER,
                &rank,
                label.clone(),
                INK,
            );
            painter.text(
                Pos2::new(outer.right() - margin * 0.5, y),
                Align2::CENTER_CENTER,
                &rank,
                label.clone(),
                INK,
            );
        }

        let (black, white) = self.position.black_white();
        let progress = self.flip_progress();
        let flipping = self.flip.as_ref().map_or(0, |flip| flip.squares);
        for square in 0..64u8 {
            let bit = 1u64 << square;
            if (black | white) & bit == 0 {
                continue;
            }
            let center = cell_rect(inner, cell, square % 8, 7 - square / 8).center();
            let mut is_black = black & bit != 0;
            let mut squash = 1.0;
            if let (Some(t), true) = (progress, flipping & bit != 0) {
                // Show the old colour for the first half, then grow back in the new one.
                if t < 0.5 {
                    is_black = !is_black;
                }
                squash = (2.0 * t - 1.0).abs().max(0.08);
            }
            disc(&painter, center, cell * 0.41, squash, is_black);
        }
        let human_turn =
            self.human_to_move() && self.thinking.is_none() && !is_terminal(self.position.board);
        if human_turn && self.show_frontier {
            let (black, white) = self.position.black_white();
            let human = if self.human_black { black } else { white };
            for square in squares(frontier(self.position.board) & human) {
                let center = cell_rect(inner, cell, square % 8, 7 - square / 8).center();
                painter.circle_stroke(
                    center,
                    cell * 0.46,
                    Stroke::new(3.0, Color32::from_rgb(245, 190, 70)),
                );
            }
        }
        if let Some(square) = self.last_move {
            let center = cell_rect(inner, cell, square % 8, 7 - square / 8).center();
            painter.circle_filled(center, cell * 0.07, Color32::from_rgb(230, 70, 60));
        }

        if !human_turn {
            return;
        }
        let moves = self.position.moves();
        let hover = response
            .hover_pos()
            .and_then(|pos| square_at(inner, cell, pos));
        let hint = if self.human_black {
            Color32::from_black_alpha(70)
        } else {
            Color32::from_white_alpha(70)
        };
        for square in 0..64u8 {
            if moves & (1u64 << square) == 0 {
                continue;
            }
            let rect = cell_rect(inner, cell, square % 8, 7 - square / 8);
            if hover == Some(square) {
                painter.rect_filled(
                    rect.shrink(2.0),
                    CornerRadius::same(4),
                    Color32::from_white_alpha(28),
                );
                disc(&painter, rect.center(), cell * 0.41, 1.0, self.human_black);
            } else {
                painter.circle_filled(rect.center(), cell * 0.12, hint);
            }
        }
        if self.show_move_counts {
            let counts = opponent_move_counts(self.position.board);
            for square in squares(moves) {
                let count = counts[square as usize];
                if count == NO_MOVE {
                    continue;
                }
                let rect = cell_rect(inner, cell, square % 8, 7 - square / 8);
                let color = if count == 0 {
                    Color32::from_rgb(255, 190, 90)
                } else {
                    INK
                };
                painter.circle_filled(rect.center(), cell * 0.19, Color32::from_black_alpha(150));
                painter.text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    count.to_string(),
                    FontId::proportional(cell * 0.28),
                    color,
                );
                if count == 0 {
                    painter.circle_stroke(rect.center(), cell * 0.25, Stroke::new(2.0, color));
                }
            }
        }
        if response.clicked()
            && let Some(square) = hover.filter(|&square| moves & (1u64 << square) != 0)
        {
            self.apply(square);
            self.message = format!("You played {}.", name(square));
        }
    }

    fn panel_ui(&mut self, ui: &mut egui::Ui) {
        ui.add_space(8.0);
        ui.heading(RichText::new("Othello").size(28.0).strong());
        ui.label(RichText::new("you vs. the engine").weak());
        ui.add_space(12.0);

        let (black, white) = self.position.black_white();
        ui.horizontal(|ui| {
            score_chip(ui, true, black.count_ones(), self.human_black);
            ui.add_space(12.0);
            score_chip(ui, false, white.count_ones(), !self.human_black);
        });
        ui.add_space(12.0);

        let turn = if is_terminal(self.position.board) {
            "Game over".to_string()
        } else if self.thinking.is_some() {
            "Engine is thinking…".to_string()
        } else if self.human_to_move() {
            "Your turn".to_string()
        } else {
            "Engine to move".to_string()
        };
        ui.horizontal(|ui| {
            ui.label(RichText::new(turn).size(18.0));
            if let Some(thinking) = &self.thinking {
                ui.add(egui::Spinner::new());
                ui.label(format!("{:.1}s", thinking.started.elapsed().as_secs_f32()));
            }
        });
        ui.label(RichText::new(&self.message).italics());
        ui.add_space(10.0);

        if let Some((found, elapsed, engine_black)) = self.last_search {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.label(RichText::new("Last engine search").strong());
                let side = if engine_black { "Black" } else { "White" };
                let discs = found.score as f32 / 100.0;
                if found.solved {
                    let outcome = match found.score.signum() {
                        1 => format!("{side} wins by {:.0}", discs),
                        -1 => format!("{side} loses by {:.0}", -discs),
                        _ => "draw".into(),
                    };
                    ui.label(format!("Solved exactly: {outcome}"));
                } else {
                    ui.label(format!(
                        "Depth {} · eval {:+.1} discs for {side}",
                        found.depth, discs
                    ));
                }
                ui.label(format!(
                    "{} nodes in {:.2}s",
                    found.nodes,
                    elapsed.as_secs_f32()
                ));
            });
            ui.add_space(10.0);
        }

        ui.separator();
        ui.label(RichText::new("Engine strength").strong());
        ui.add(
            egui::Slider::new(&mut self.think_seconds, 0.1..=10.0)
                .logarithmic(true)
                .suffix(" s / move")
                .text("think time"),
        );
        ui.add(egui::Slider::new(&mut self.threshold, 8..=16).text("solve at empties"));
        if self.threshold > 14 {
            ui.label(
                RichText::new("High solve thresholds need generous think time.")
                    .small()
                    .weak(),
            );
        }
        ui.add_space(10.0);
        ui.separator();
        ui.label(RichText::new("Learning aids").strong());
        ui.checkbox(&mut self.show_move_counts, "Opponent move counts");
        ui.checkbox(&mut self.show_frontier, "Frontier discs");
        ui.label(
            RichText::new("Prefer moves that leave you few discs touching empty squares.")
                .small()
                .weak(),
        );
        ui.add_space(10.0);
        ui.separator();
        ui.horizontal(|ui| {
            if ui.button("New game as Black").clicked() {
                self.new_game(true);
            }
            if ui.button("New game as White").clicked() {
                self.new_game(false);
            }
        });
        let can_undo = self
            .history
            .iter()
            .any(|(p, _)| p.black_to_move == self.human_black);
        if ui
            .add_enabled(can_undo, egui::Button::new("Take back move"))
            .clicked()
        {
            self.undo();
        }
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let busy = self.step();
        if busy || self.thinking.is_some() || self.flip_progress().is_some() {
            ctx.request_repaint_after(Duration::from_millis(16));
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Panel::right("panel")
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(32, 36, 42))
                    .inner_margin(18.0),
            )
            .exact_size(320.0)
            .show(ui, |ui| {
                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(18, 12))
                    .show(ui, |ui| self.panel_ui(ui));
            });
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(24, 28, 32))
                    .inner_margin(16.0),
            )
            .show(ui, |ui| ui.centered_and_justified(|ui| self.board_ui(ui)));
    }
}

fn cell_rect(inner: Rect, cell: f32, file: u8, row: u8) -> Rect {
    Rect::from_min_size(
        inner.left_top() + Vec2::new(file as f32, row as f32) * cell,
        Vec2::splat(cell),
    )
}

fn square_at(inner: Rect, cell: f32, pos: Pos2) -> Option<u8> {
    if !inner.contains(pos) {
        return None;
    }
    let file = ((pos.x - inner.left()) / cell) as u8;
    let row = ((pos.y - inner.top()) / cell) as u8;
    (file < 8 && row < 8).then(|| (7 - row) * 8 + file)
}

fn name(square: u8) -> String {
    format!("{}{}", (b'a' + square % 8) as char, square / 8 + 1)
}

/// A shaded disc; `squash` scales it horizontally for the flip animation.
fn disc(painter: &egui::Painter, center: Pos2, radius: f32, squash: f32, black: bool) {
    let (base, rim, shine) = if black {
        (
            Color32::from_rgb(28, 28, 32),
            Color32::from_rgb(8, 8, 10),
            Color32::from_white_alpha(38),
        )
    } else {
        (
            Color32::from_rgb(238, 236, 228),
            Color32::from_rgb(170, 168, 160),
            Color32::from_white_alpha(150),
        )
    };
    let ellipse = |c: Pos2, r: f32, color: Color32| {
        if squash >= 0.999 {
            painter.circle_filled(c, r, color);
        } else {
            let points = (0..32)
                .map(|i| {
                    let a = i as f32 / 32.0 * std::f32::consts::TAU;
                    c + Vec2::new(a.cos() * r * squash, a.sin() * r)
                })
                .collect();
            painter.add(egui::Shape::convex_polygon(points, color, Stroke::NONE));
        }
    };
    ellipse(
        center + Vec2::new(radius * 0.06, radius * 0.1),
        radius,
        Color32::from_black_alpha(90),
    );
    ellipse(center, radius, rim);
    ellipse(center - Vec2::new(0.0, radius * 0.04), radius * 0.9, base);
    ellipse(
        center - Vec2::new(radius * 0.22 * squash, radius * 0.28),
        radius * 0.38,
        shine,
    );
}

fn score_chip(ui: &mut egui::Ui, black: bool, count: u32, you: bool) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(28.0, 28.0), Sense::hover());
    disc(ui.painter(), rect.center(), 12.0, 1.0, black);
    ui.vertical(|ui| {
        ui.label(RichText::new(count.to_string()).size(22.0).strong());
        ui.label(
            RichText::new(if you { "you" } else { "engine" })
                .small()
                .weak(),
        );
    });
}
