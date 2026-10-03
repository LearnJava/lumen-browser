//! CSS Gap Decorations L1 §4.5–§4.6 — списки значений `*-rule-width/-style/-color` с
//! нотацией `repeat()` и раздача значений щелям контейнера.
//!
//! Список — это `<value>#` либо `<value>#? , repeat(auto, <value>#) , <value>#?`; элемент
//! списка — одно значение или `repeat(<integer [1,∞]>, <value>#)`. Исходная форма
//! сохраняется (computed value = as specified), а раскрытие в плоский ряд делается
//! при раздаче значений щелям ([`RuleList::value_for_gap`]).

/// Элемент списка: одиночное значение или повторитель.
#[derive(Debug, Clone, PartialEq)]
pub enum RuleItem<T> {
    /// Одно значение.
    One(T),
    /// `repeat(<integer>, <value>#)` — значения второго аргумента `N` раз подряд.
    Repeat(u32, Vec<T>),
    /// `repeat(auto, <value>#)` — заполняет щели, не получившие значения от прочих
    /// элементов. В списке не более одного.
    Auto(Vec<T>),
}

/// Список значений одного `*-rule-{width,style,color}` по щелям одной оси.
#[derive(Debug, Clone, PartialEq)]
pub struct RuleList<T> {
    items: Vec<RuleItem<T>>,
}

impl<T: Clone> RuleList<T> {
    /// Список из одного значения — initial всех трёх свойств и результат `rule: 1px solid`.
    pub fn single(v: T) -> Self {
        Self {
            items: vec![RuleItem::One(v)],
        }
    }

    /// Исходные элементы списка (для сериализации).
    pub fn items(&self) -> &[RuleItem<T>] {
        &self.items
    }

    /// Первое значение списка: то, что получает первая щель (и единственное значение
    /// «простого» списка). Multicol-код до появления списков читал именно его.
    pub fn first(&self) -> &T {
        match &self.items[0] {
            RuleItem::One(v) => v,
            RuleItem::Repeat(_, vs) | RuleItem::Auto(vs) => &vs[0],
        }
    }

    /// Все значения без раскрытия повторителей (для проверок «хоть одно видимо»).
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.items
            .iter()
            .flat_map(|it| -> Box<dyn Iterator<Item = &T> + '_> {
                match it {
                    RuleItem::One(v) => Box::new(std::iter::once(v)),
                    RuleItem::Repeat(_, vs) | RuleItem::Auto(vs) => Box::new(vs.iter()),
                }
            })
    }

    /// Применяет `f` к каждому хранимому значению.
    pub fn for_each_mut(&mut self, mut f: impl FnMut(&mut T)) {
        for it in &mut self.items {
            match it {
                RuleItem::One(v) => f(v),
                RuleItem::Repeat(_, vs) | RuleItem::Auto(vs) => vs.iter_mut().for_each(&mut f),
            }
        }
    }

    /// Поэлементное отображение с сохранением формы списка.
    pub fn map<U>(&self, mut f: impl FnMut(&T) -> U) -> RuleList<U> {
        let items = self
            .items
            .iter()
            .map(|it| match it {
                RuleItem::One(v) => RuleItem::One(f(v)),
                RuleItem::Repeat(n, vs) => RuleItem::Repeat(*n, vs.iter().map(&mut f).collect()),
                RuleItem::Auto(vs) => RuleItem::Auto(vs.iter().map(&mut f).collect()),
            })
            .collect();
        RuleList { items }
    }

    /// §4.6 — значение для щели `index` из `total` щелей оси.
    pub fn value_for_gap(&self, index: usize, total: usize) -> &T {
        // Раскрытие целочисленных повторителей: ряд значений до и после `auto`.
        let mut leading: Vec<&T> = Vec::new();
        let mut trailing: Vec<&T> = Vec::new();
        let mut auto: Option<&Vec<T>> = None;
        for it in &self.items {
            let dst = if auto.is_some() {
                &mut trailing
            } else {
                &mut leading
            };
            match it {
                RuleItem::One(v) => dst.push(v),
                RuleItem::Repeat(n, vs) => {
                    for _ in 0..*n {
                        dst.extend(vs.iter());
                    }
                }
                RuleItem::Auto(vs) => auto = Some(vs),
            }
        }
        let Some(auto) = auto else {
            // Без `auto`: значения идут по кругу, пока не кончатся щели.
            return leading[index % leading.len()];
        };
        let trailing_gaps = trailing.len().min(total.saturating_sub(leading.len()));
        let trailing_start = total.saturating_sub(trailing_gaps);
        if index < leading.len() {
            leading[index]
        } else if index >= trailing_start {
            trailing[index - trailing_start]
        } else {
            &auto[(index - leading.len()) % auto.len()]
        }
    }

    /// Разбирает `<list>`: значения через запятую вне скобок, `repeat(N|auto, v#)`.
    /// `item` разбирает одно значение (`<line-width>`, `<line-style>`, `<color>` или целое
    /// `<gap-rule>`). `None` — невалидно: пустой элемент, `repeat(0, …)`, два `auto`,
    /// вложенный `repeat()`.
    pub fn parse(val: &str, item: &impl Fn(&str) -> Option<T>) -> Option<Self> {
        let mut items = Vec::new();
        let mut has_auto = false;
        for part in split_commas(val) {
            let part = part.trim();
            if part.is_empty() {
                return None;
            }
            match parse_repeat(part, item)? {
                Some((count, vs)) => match count {
                    Some(n) => items.push(RuleItem::Repeat(n, vs)),
                    None => {
                        if std::mem::replace(&mut has_auto, true) {
                            return None;
                        }
                        items.push(RuleItem::Auto(vs));
                    }
                },
                None => items.push(RuleItem::One(item(part)?)),
            }
        }
        (!items.is_empty()).then_some(Self { items })
    }

    /// Сериализация computed value: исходная форма с `repeat()`.
    pub fn to_css(&self, f: impl Fn(&T) -> String) -> String {
        let join = |vs: &[T]| vs.iter().map(&f).collect::<Vec<_>>().join(", ");
        self.items
            .iter()
            .map(|it| match it {
                RuleItem::One(v) => f(v),
                RuleItem::Repeat(n, vs) => format!("repeat({n}, {})", join(vs)),
                RuleItem::Auto(vs) => format!("repeat(auto, {})", join(vs)),
            })
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// `repeat(<count>, v#)` → `Some((Some(n) | None для auto, значения))`; не `repeat(` → `None`;
/// `repeat(` с ошибкой в аргументах → внешний `None`.
#[allow(clippy::type_complexity)]
fn parse_repeat<T>(
    part: &str,
    item: &impl Fn(&str) -> Option<T>,
) -> Option<Option<(Option<u32>, Vec<T>)>> {
    let lower = part.get(..7).map(str::to_ascii_lowercase);
    if lower.as_deref() != Some("repeat(") {
        return Some(None);
    }
    let inner = part[7..].strip_suffix(')')?;
    let args = split_commas(inner);
    let (count, rest) = args.split_first()?;
    let count = count.trim();
    let n = if count.eq_ignore_ascii_case("auto") {
        None
    } else {
        // `<integer [1,∞]>`: только цифры, без знака и дробей.
        if count.is_empty() || !count.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        Some(count.parse::<u32>().ok().filter(|&n| n >= 1)?)
    };
    if rest.is_empty() {
        return None;
    }
    let vs = rest
        .iter()
        .map(|s| {
            let s = s.trim();
            // Вложенный repeat() запрещён: `item` его не примет.
            item(s)
        })
        .collect::<Option<Vec<T>>>()?;
    Some(Some((n, vs)))
}

/// Делит по запятым вне скобок.
fn split_commas(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                out.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&s[start..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn num(s: &str) -> Option<i32> {
        s.trim().parse().ok()
    }

    fn list(s: &str) -> Option<RuleList<i32>> {
        RuleList::parse(s, &num)
    }

    fn assign(l: &RuleList<i32>, total: usize) -> Vec<i32> {
        (0..total).map(|i| *l.value_for_gap(i, total)).collect()
    }

    #[test]
    fn plain_list_cycles() {
        let l = list("1, 2, 3").unwrap();
        assert_eq!(assign(&l, 7), [1, 2, 3, 1, 2, 3, 1]);
        assert_eq!(*l.first(), 1);
    }

    #[test]
    fn integer_repeat_expands() {
        let l = list("9, repeat(3, 1, 2), 9").unwrap();
        assert_eq!(assign(&l, 8), [9, 1, 2, 1, 2, 1, 2, 9]);
        assert_eq!(l.to_css(|v| v.to_string()), "9, repeat(3, 1, 2), 9");
    }

    #[test]
    fn auto_repeat_fills_the_middle() {
        let l = list("9, repeat(auto, 1, 2), 8").unwrap();
        assert_eq!(assign(&l, 6), [9, 1, 2, 1, 2, 8]);
        assert_eq!(assign(&l, 3), [9, 1, 8]);
        // Меньше щелей, чем значений вне auto: ведущие побеждают, хвост обрезается спереди.
        assert_eq!(assign(&l, 2), [9, 8]);
        assert_eq!(assign(&l, 1), [9]);
    }

    #[test]
    fn auto_repeat_only_and_trailing_only() {
        let l = list("repeat(auto, 1, 2)").unwrap();
        assert_eq!(assign(&l, 5), [1, 2, 1, 2, 1]);
        let l = list("repeat(auto, 1), 5, 6").unwrap();
        assert_eq!(assign(&l, 5), [1, 1, 1, 5, 6]);
    }

    #[test]
    fn invalid_lists_are_rejected() {
        for bad in [
            "",
            "1,",
            ",1",
            "1,,2",
            "repeat(0, 1)",
            "repeat(-1, 1)",
            "repeat(1.5, 1)",
            "repeat(2)",
            "repeat(2,)",
            "repeat(auto, 1), repeat(auto, 2)",
            "repeat(2, repeat(2, 1))",
            "repeat(2, 1",
            "1 2",
            "repeat(x, 1)",
        ] {
            assert!(list(bad).is_none(), "{bad:?} must be invalid");
        }
    }

    #[test]
    fn map_keeps_shape_and_iter_sees_all() {
        let l = list("1, repeat(2, 2, 3), repeat(auto, 4)").unwrap();
        assert_eq!(l.iter().copied().collect::<Vec<_>>(), [1, 2, 3, 4]);
        let m = l.map(|v| v * 10);
        assert_eq!(
            m.to_css(|v| v.to_string()),
            "10, repeat(2, 20, 30), repeat(auto, 40)"
        );
    }
}
