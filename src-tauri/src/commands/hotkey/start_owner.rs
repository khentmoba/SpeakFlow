use parking_lot::Mutex;

#[derive(Default)]
pub(crate) struct HotkeyStartOwner {
    inner: Mutex<StartOwnership>,
}

#[derive(Default)]
struct StartOwnership {
    generation: u64,
    active: bool,
    session_id: Option<u64>,
}

impl HotkeyStartOwner {
    pub(crate) fn begin(&self) -> u64 {
        let mut inner = self.inner.lock();
        inner.generation = inner.generation.wrapping_add(1);
        inner.active = true;
        inner.session_id = None;
        inner.generation
    }

    pub(crate) fn bind(&self, generation: u64, session_id: u64) -> bool {
        let mut inner = self.inner.lock();
        if !inner.active || inner.generation != generation || inner.session_id.is_some() {
            return false;
        }
        inner.session_id = Some(session_id);
        true
    }

    pub(crate) fn release(&self) -> Option<u64> {
        let mut inner = self.inner.lock();
        inner.active = false;
        inner.session_id.take()
    }

    pub(crate) fn fail(&self, generation: u64) -> bool {
        let mut inner = self.inner.lock();
        if !inner.active || inner.generation != generation {
            return false;
        }
        inner.active = false;
        inner.session_id = None;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::HotkeyStartOwner;

    #[test]
    fn release_before_async_start_rejects_late_bind() {
        let owner = HotkeyStartOwner::default();
        let generation = owner.begin();

        assert_eq!(owner.release(), None);
        assert!(!owner.bind(generation, 41));
        assert_eq!(owner.release(), None);
    }

    #[test]
    fn old_generation_cannot_bind_after_a_new_begin() {
        let owner = HotkeyStartOwner::default();
        let old_generation = owner.begin();
        let new_generation = owner.begin();

        assert!(!owner.bind(old_generation, 101));
        assert!(owner.bind(new_generation, 202));
        assert_eq!(owner.release(), Some(202));
    }

    #[test]
    fn old_generation_failure_cannot_cancel_new_generation() {
        let owner = HotkeyStartOwner::default();
        let old_generation = owner.begin();
        let new_generation = owner.begin();

        assert!(!owner.fail(old_generation));
        assert!(owner.bind(new_generation, 202));
        assert_eq!(owner.release(), Some(202));
    }

    #[test]
    fn duplicate_bind_does_not_replace_the_owned_session() {
        let owner = HotkeyStartOwner::default();
        let generation = owner.begin();

        assert!(owner.bind(generation, 101));
        assert!(!owner.bind(generation, 202));
        assert_eq!(owner.release(), Some(101));
    }

    #[test]
    fn registration_gates_have_independent_session_ownership() {
        let first_gate = HotkeyStartOwner::default();
        let second_gate = HotkeyStartOwner::default();
        let first_generation = first_gate.begin();
        let second_generation = second_gate.begin();

        assert!(first_gate.bind(first_generation, 101));
        assert!(second_gate.bind(second_generation, 202));
        assert_eq!(first_gate.release(), Some(101));
        assert_eq!(second_gate.release(), Some(202));
    }
}
