use super::*;

/// Every own property of JavaScript's `Object.prototype`, as `Object.getOwnPropertyNames` lists it.
const OBJECT_PROTOTYPE: [&str; 12] = [
    "constructor",
    "__defineGetter__",
    "__defineSetter__",
    "hasOwnProperty",
    "__lookupGetter__",
    "__lookupSetter__",
    "isPrototypeOf",
    "propertyIsEnumerable",
    "toString",
    "valueOf",
    "__proto__",
    "toLocaleString",
];

/// The locals of a stop are kept by name whatever the name, also each name `Object.prototype` has, with their
/// values.
#[test]
fn retains_locals_whose_names_are_object_prototype_properties() {
    let mut world = java_session();
    world.stopped(1, 5, "step");
    let locals = Reply::Variables(OBJECT_PROTOTYPE.iter().map(|name| variable(name, "known")).collect());
    pump(&mut world, &mut VecDeque::new(), &mut VecDeque::from([locals]));
    let kept = &world.state.execution_point().locals;
    assert_eq!(kept.len(), OBJECT_PROTOTYPE.len(), "{kept:?}");
    for name in OBJECT_PROTOTYPE {
        assert_eq!(kept.get(name).map(|local| local.value.as_str()), Some("known"), "{name}");
    }
}
