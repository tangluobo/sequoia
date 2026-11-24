use super::temporal_model::{Date, Kind, Value};
pub struct Picker {
    pub value: Value,
    pub fields: [String; 3],
    pub editors: [crate::tabs::text_state::Editor; 3],
    pub anchor: egui::Pos2,
    pub focused_once: bool,
}
impl Picker {
    pub fn new(kind: Kind, text: &str, anchor: egui::Pos2) -> Self {
        use chrono::Datelike;
        let today = chrono::Local::now().date_naive();
        let value = Value::new(
            kind,
            text,
            Date {
                year: today.year(),
                month: today.month(),
                day: today.day(),
            },
        );
        let fields = value.time.map(|value| value.to_string());
        Self {
            value,
            fields,
            editors: Default::default(),
            anchor,
            focused_once: false,
        }
    }
    pub fn height(&self) -> f32 {
        let weeks = (self.value.cursor.weekday() + self.value.cursor.days()).div_ceil(7);
        let calendar = if self.value.kind == Kind::Time {
            0.
        } else {
            36. + weeks as f32 * 27. + 8.
        };
        22. + calendar
            + if self.value.kind == Kind::Date {
                0.
            } else {
                38.
            }
            + 23.
    }
    pub fn commit_time(&mut self, index: usize) {
        if let Ok(value) = self.fields[index]
            .trim_matches(|ch| ch <= ' ')
            .parse::<i32>()
        {
            self.value.time[index] = value.clamp(0, if index == 0 { 23 } else { 59 }) as u32;
        }
        self.fields[index] = self.value.time[index].to_string();
    }
    pub fn release(&self, ctx: &egui::Context) {
        for editor in &self.editors {
            editor.release(ctx);
        }
    }
}
