//! Sub-step reporting for long nodes. A host (the web worker) installs a hook; nodes call `step` between phases.
//! The hook is per thread, so calls from rayon workers are ignored: report only from the thread running the node.
use std::cell::RefCell;
use std::rc::Rc;

type Hook = Rc<dyn Fn(&str, f64)>;

thread_local! {
    static HOOK: RefCell<Option<Hook>> = const { RefCell::new(None) };
}

pub fn set_hook(h: Option<Hook>) {
    HOOK.with(|x| *x.borrow_mut() = h);
}

/// A phase just finished: `label` names it, `frac` (0..=1) is how much of the node is now done.
pub fn step(label: &str, frac: f64) {
    let h = HOOK.with(|x| x.borrow().clone());
    if let Some(h) = h {
        h(label, frac.clamp(0.0, 1.0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hook_sees_steps_until_removed() {
        let seen = Rc::new(RefCell::new(Vec::new()));
        let s = seen.clone();
        set_hook(Some(Rc::new(move |l, f| {
            s.borrow_mut().push((l.to_string(), f))
        })));
        step("a", 0.5);
        step("b", 2.0);
        set_hook(None);
        step("c", 1.0);
        assert_eq!(
            *seen.borrow(),
            vec![("a".to_string(), 0.5), ("b".to_string(), 1.0)]
        );
    }
}
