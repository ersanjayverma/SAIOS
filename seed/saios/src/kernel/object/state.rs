#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum ObjectState {
    Created,
    Initializing,
    Ready,
    Stopping,
    Destroyed,
}

impl ObjectState {
    pub const fn as_str(self) -> &'static str {
        match self {
            ObjectState::Created => "Created",
            ObjectState::Initializing => "Initializing",
            ObjectState::Ready => "Ready",
            ObjectState::Stopping => "Stopping",
            ObjectState::Destroyed => "Destroyed",
        }
    }

    pub fn can_transition(self, next: ObjectState) -> bool {
        if self == next {
            return true;
        }

        matches!(
            (self, next),
            (ObjectState::Created, ObjectState::Initializing)
                | (ObjectState::Initializing, ObjectState::Ready)
                | (ObjectState::Initializing, ObjectState::Stopping)
                | (ObjectState::Ready, ObjectState::Stopping)
                | (ObjectState::Stopping, ObjectState::Destroyed)
        )
    }
}
