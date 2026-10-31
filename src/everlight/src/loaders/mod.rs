pub mod sigbypass;
use crate::sig::Target;

pub trait ModLoader {
    fn run(&self, targets: &[&Target]) -> LoadResult;
}
pub struct LoadResult {
    pub matched: usize,
    pub applied: usize,
}

impl LoadResult {
    pub fn ok(&self) -> bool {
        self.matched > 0 && self.applied == self.matched
    }

    pub fn detail(&self) -> String {
        match (self.matched, self.applied) {
            (0, _) => "no target matched".into(),
            (m, 0) => format!("all {m} matching target(s) failed"),
            (m, a) if a < m => format!("{a}/{m} target(s) applied"),
            (_, a) => format!("{a} target(s) applied"),
        }
    }
}
