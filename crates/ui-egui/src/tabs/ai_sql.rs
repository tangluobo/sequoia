#[derive(Default)]
pub struct State {
    pub generation: u64,
    pub insertion: Option<tomato_core::ai_sql::editor::Insertion>,
}
impl State {
    pub fn pending(&self) -> bool {
        self.insertion.is_some()
    }
}
