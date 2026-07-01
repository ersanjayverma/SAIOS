#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum ObjectState {
    Created,
    Running,
    Stopped,
    Faulted,
}
