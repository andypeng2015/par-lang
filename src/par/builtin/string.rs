use std::{cmp::Ordering, sync::Arc};

use arcstr::{literal, Substr};
use num_bigint::BigInt;

use crate::{
    icombs::readback::Handle,
    par::{
        builtin::{char_::CharClass, list::readback_list},
        process,
        program::{Definition, Module, TypeDef},
        types::Type,
    },
};

pub fn external_module() -> Module<Arc<process::Expression<()>>> {
    Module {
        type_defs: vec![TypeDef::external("String", &[], Type::string())],
        declarations: vec![],
        definitions: vec![
            Definition::external("Builder", Type::name(None, "Builder", vec![]), |handle| {
                Box::pin(string_builder(handle))
            }),
            Definition::external(
                "Reader",
                Type::function(
                    Type::string(),
                    Type::name(None, "Reader", vec![Type::either(vec![])]),
                ),
                |handle| Box::pin(string_reader(handle)),
            ),
            Definition::external(
                "Quote",
                Type::function(Type::string(), Type::string()),
                |handle| Box::pin(string_quote(handle)),
            ),
        ],
    }
}

async fn string_builder(mut handle: Handle) {
    let mut buf = String::new();
    loop {
        match handle.case().await.as_str() {
            "add" => {
                buf += &handle.receive().string().await;
            }
            "build" => {
                handle.provide_string(Substr::from(buf));
                break;
            }
            _ => unreachable!(),
        }
    }
}

async fn string_quote(mut handle: Handle) {
    let s = handle.receive().string().await;
    handle.provide_string(Substr::from(format!("{:?}", s)));
}

async fn string_reader(mut handle: Handle) {
    let mut remainder = handle.receive().string().await;
    loop {
        match handle.case().await.as_str() {
            "close" => {
                handle.break_();
                return;
            }
            "char" => match remainder.chars().next() {
                Some(ch) => {
                    handle.signal(literal!("char"));
                    handle.send().provide_char(ch);
                    remainder = remainder.substr(ch.len_utf8()..);
                }
                None => {
                    handle.signal(literal!("end"));
                    handle.signal(literal!("ok"));
                    handle.break_();
                    return;
                }
            },
            "match" => {
                let prefix = Pattern::readback(handle.receive()).await;
                let suffix = Pattern::readback(handle.receive()).await;
                if remainder.is_empty() {
                    handle.signal(literal!("end"));
                    handle.signal(literal!("ok"));
                    handle.break_();
                    return;
                }

                let mut m = Machine::start(Box::new(Pattern::Concat(prefix, suffix)));

                let mut best_match = None;
                for (pos, ch) in remainder.char_indices() {
                    match (m.leftmost_feasible_split(pos), best_match) {
                        (Some(fi), Some((bi, _))) if fi > bi => break,
                        (None, _) => break,
                        _ => {}
                    }
                    m.advance(pos, ch.len_utf8(), ch);
                    match (m.leftmost_accepting_split(), best_match) {
                        (Some(ai), Some((bi, _))) if ai <= bi => {
                            best_match = Some((ai, pos + ch.len_utf8()))
                        }
                        (Some(ai), None) => best_match = Some((ai, pos + ch.len_utf8())),
                        _ => {}
                    }
                }

                match best_match {
                    Some((i, j)) => {
                        handle.signal(literal!("match"));
                        handle.send().provide_string(remainder.substr(..i));
                        handle.send().provide_string(remainder.substr(i..j));
                        remainder = remainder.substr(j..);
                    }
                    None => {
                        handle.signal(literal!("fail"));
                    }
                }
            }
            "matchEnd" => {
                let prefix = Pattern::readback(handle.receive()).await;
                let suffix = Pattern::readback(handle.receive()).await;
                if remainder.is_empty() {
                    handle.signal(literal!("end"));
                    handle.signal(literal!("ok"));
                    handle.break_();
                    return;
                }

                let mut m = Machine::start(Box::new(Pattern::Concat(prefix, suffix)));

                for (pos, ch) in remainder.char_indices() {
                    if m.accepts() == None {
                        break;
                    }
                    m.advance(pos, ch.len_utf8(), ch);
                }

                match m.leftmost_accepting_split() {
                    Some(i) => {
                        handle.signal(literal!("match"));
                        handle.send().provide_string(remainder.substr(..i));
                        handle.send().provide_string(remainder.substr(i..));
                        handle.break_();
                        return;
                    }
                    None => {
                        handle.signal(literal!("fail"));
                    }
                }
            }
            "remainder" => {
                handle.signal(literal!("ok"));
                handle.provide_string(remainder);
                return;
            }
            _ => unreachable!(),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) enum Pattern {
    Nil,
    All,
    Empty,
    Length(BigInt),
    Str(Substr),
    One(CharClass),
    Non(CharClass),
    Concat(Box<Self>, Box<Self>),
    And(Box<Self>, Box<Self>),
    Or(Box<Self>, Box<Self>),
    Repeat(Box<Self>),
    Repeat1(Box<Self>),
}

impl Pattern {
    pub(crate) async fn readback(mut handle: Handle) -> Box<Self> {
        match handle.case().await.as_str() {
            "and" => {
                // .and List<self>
                let mut conj = Box::new(Self::All);
                let patterns =
                    readback_list(handle, |handle| Box::pin(Self::readback(handle))).await;
                for p in patterns.into_iter().rev() {
                    conj = Box::new(Self::And(p, conj));
                }
                conj
            }
            "concat" => {
                // .concat List<self>
                let mut conc = Box::new(Self::Empty);
                let patterns =
                    readback_list(handle, |handle| Box::pin(Self::readback(handle))).await;
                for p in patterns.into_iter().rev() {
                    conc = Box::new(Self::Concat(p, conc));
                }
                conc
            }
            "empty" => {
                // .empty!
                handle.break_();
                Box::new(Self::Empty)
            }
            "length" => {
                // .length Nat
                let n = handle.nat().await;
                Box::new(Self::Length(n))
            }
            "non" => {
                // .non Char.Class
                let class = CharClass::readback(handle).await;
                Box::new(Self::Non(class))
            }
            "one" => {
                // .one Char.Class
                let class = CharClass::readback(handle).await;
                Box::new(Self::One(class))
            }
            "or" => {
                // .or List<self>,
                let mut disj = Box::new(Self::Nil);
                let patterns =
                    readback_list(handle, |handle| Box::pin(Self::readback(handle))).await;
                for p in patterns.into_iter().rev() {
                    disj = Box::new(Self::Or(p, disj));
                }
                disj
            }
            "repeat" => {
                // .repeat self
                let p = Box::pin(Self::readback(handle)).await;
                Box::new(Self::Repeat(p))
            }
            "repeat1" => {
                // .repeat1 self
                let p = Box::pin(Self::readback(handle)).await;
                Box::new(Self::Repeat1(p))
            }
            "str" => {
                // .str String
                let s = handle.string().await;
                Box::new(Self::Str(s))
            }
            _ => unreachable!(),
        }
    }
}

#[derive(Debug)]
pub(crate) struct Machine {
    pattern: Box<Pattern>,
    inner: MachineInner,
}

impl Machine {
    pub(crate) fn start(pattern: Box<Pattern>) -> Self {
        let inner = MachineInner::start(&pattern, 0);
        Self { pattern, inner }
    }

    pub(crate) fn accepts(&self) -> Option<bool> {
        self.inner.accepts(&self.pattern)
    }

    pub(crate) fn advance(&mut self, pos: usize, len: usize, ch: char) {
        self.inner.advance(&self.pattern, pos, len, ch);
    }

    pub(crate) fn leftmost_accepting_split(&self) -> Option<usize> {
        let Pattern::Concat(_, p2) = self.pattern.as_ref() else {
            return None;
        };
        let State::Concat(_, heap) = &self.inner.state else {
            return None;
        };
        heap.iter()
            .filter(|m2| m2.accepts(p2) == Some(true))
            .map(|m2| m2.start)
            .min()
    }

    pub(crate) fn leftmost_feasible_split(&self, pos: usize) -> Option<usize> {
        let State::Concat(_, heap) = &self.inner.state else {
            return None;
        };
        heap.iter().map(|m2| m2.start).min().or(Some(pos))
    }
}

#[derive(Debug)]
struct MachineInner {
    state: State,
    start: usize,
}

impl MachineInner {
    fn start(pattern: &Pattern, start: usize) -> Self {
        let state = match pattern {
            Pattern::Nil => State::Halt,

            Pattern::All => State::Init,

            Pattern::Empty => State::Init,

            Pattern::Length(_) => State::Index(0),

            Pattern::Str(_) => State::Index(0),

            Pattern::One(_) => State::Index(0),
            Pattern::Non(_) => State::Index(0),

            Pattern::Concat(p1, p2) => {
                let prefix = Self::start(p1, start);
                let suffixes = if prefix.accepts(p1) == Some(true) {
                    vec![Self::start(p2, start)]
                } else {
                    vec![]
                };
                State::Concat(Box::new(prefix), suffixes)
            }

            Pattern::And(p1, p2) | Pattern::Or(p1, p2) => State::Pair(
                Box::new(Self::start(p1, start)),
                Box::new(Self::start(p2, start)),
            ),

            Pattern::Repeat(_) => State::Init,
            Pattern::Repeat1(p) => State::Heap(vec![Self::start(p, start)]),
        };

        Self { state, start }
    }

    fn accepts(&self, pattern: &Pattern) -> Option<bool> {
        match (pattern, &self.state) {
            (_, State::Halt) => None,

            (Pattern::All, State::Init) => Some(true),

            (Pattern::Empty, State::Init) => Some(true),

            (Pattern::Length(n), State::Index(i)) => Some(n == &BigInt::from(*i)),

            (Pattern::Str(s), State::Index(i)) => Some(s.len() == *i),

            (Pattern::One(_), State::Index(i)) => Some(*i == 1),
            (Pattern::Non(_), State::Index(i)) => Some(*i == 1),

            (Pattern::Concat(p1, p2), State::Concat(m1, heap)) => heap
                .iter()
                .filter_map(|m2| m2.accepts(p2))
                .max()
                .or_else(|| m1.accepts(p1).map(|_| false)),

            (Pattern::And(p1, p2), State::Pair(m1, m2)) => match (m1.accepts(p1), m2.accepts(p2)) {
                (Some(a1), Some(a2)) => Some(a1 && a2),
                (None, _) | (_, None) => None,
            },

            (Pattern::Or(p1, p2), State::Pair(m1, m2)) => match (m1.accepts(p1), m2.accepts(p2)) {
                (Some(a1), Some(a2)) => Some(a1 || a2),
                (None, a) | (a, None) => a,
            },

            (Pattern::Repeat(_), State::Init) => Some(true),
            (Pattern::Repeat(p), State::Heap(heap)) => {
                heap.iter().filter_map(|m| m.accepts(p)).max()
            }

            (Pattern::Repeat1(p), State::Heap(heap)) => {
                heap.iter().filter_map(|m| m.accepts(p)).max()
            }

            (p, s) => unreachable!("invalid combination of pattern {:?} and state {:?}", p, s),
        }
    }

    fn advance(&mut self, pattern: &Pattern, pos: usize, len: usize, ch: char) {
        match (pattern, &mut self.state) {
            (_, State::Halt) => {}

            (Pattern::All, State::Init) => {}

            (Pattern::Empty, State::Init) => self.state = State::Halt,

            (Pattern::Length(n), State::Index(i)) => {
                if &BigInt::from(*i) < n {
                    *i += 1;
                } else {
                    self.state = State::Halt;
                }
            }

            (Pattern::Str(s), State::Index(i)) => {
                if s.substr(*i..).chars().next() == Some(ch) {
                    *i += ch.len_utf8();
                } else {
                    self.state = State::Halt;
                }
            }

            (Pattern::One(class), State::Index(i)) => {
                if *i == 0 && class.contains(ch) {
                    *i = 1;
                } else {
                    self.state = State::Halt;
                }
            }
            (Pattern::Non(class), State::Index(i)) => {
                if *i == 0 && !class.contains(ch) {
                    *i = 1;
                } else {
                    self.state = State::Halt;
                }
            }

            (Pattern::Concat(p1, p2), State::Concat(m1, heap)) => {
                m1.advance(p1, pos, len, ch);
                for m2 in heap.iter_mut() {
                    m2.advance(p2, pos, len, ch);
                }
                heap.retain(|m2| m2.state != State::Halt);
                if m1.accepts(p1) == Some(true) {
                    heap.push(Self::start(p2, pos + len));
                }
                heap.sort_by_key(|m| m.start);
                heap.sort();
                heap.dedup();
                if m1.state == State::Halt && heap.is_empty() {
                    self.state = State::Halt;
                }
            }

            (Pattern::And(p1, p2), State::Pair(m1, m2)) => {
                m1.advance(p1, pos, len, ch);
                m2.advance(p2, pos, len, ch);
                if m1.state == State::Halt || m2.state == State::Halt {
                    self.state = State::Halt;
                }
            }

            (Pattern::Or(p1, p2), State::Pair(m1, m2)) => {
                m1.advance(p1, pos, len, ch);
                m2.advance(p2, pos, len, ch);
                if m1.state == State::Halt && m2.state == State::Halt {
                    self.state = State::Halt;
                }
            }

            (Pattern::Repeat(p), State::Init) => {
                let mut m = Self::start(p, pos);
                m.advance(p, pos, len, ch);
                self.state = State::Heap(vec![m])
            }
            (Pattern::Repeat(p) | Pattern::Repeat1(p), State::Heap(heap)) => {
                if heap.iter().any(|m| m.accepts(p) == Some(true)) {
                    heap.push(Self::start(p, pos));
                }
                for m in heap.iter_mut() {
                    m.advance(p, pos, len, ch);
                }
                heap.retain(|m| m.state != State::Halt);
                heap.sort_by_key(|m| m.start);
                heap.sort();
                heap.dedup();
                if heap.is_empty() {
                    self.state = State::Halt;
                }
            }

            (p, s) => unreachable!("invalid combination of pattern {:?} and state {:?}", p, s),
        }
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
enum State {
    Init,
    Halt,
    Index(usize),
    Pair(Box<MachineInner>, Box<MachineInner>),
    Heap(Vec<MachineInner>),
    Concat(Box<MachineInner>, Vec<MachineInner>),
}

impl PartialEq for MachineInner {
    fn eq(&self, other: &Self) -> bool {
        self.state == other.state
    }
}
impl Eq for MachineInner {}
impl PartialOrd for MachineInner {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.state.partial_cmp(&other.state)
    }
}
impl Ord for MachineInner {
    fn cmp(&self, other: &Self) -> Ordering {
        self.state.cmp(&other.state)
    }
}
