use super::*;

fn source(columns: usize, rows: usize) -> TextSource {
    TextSource {
        columns: (0..columns).map(|at| GridColumn { name: format!("c{at}"), ..GridColumn::default() }).collect(),
        rows: (0..rows).map(|row| (0..columns).map(|column| format!("{row}.{column}")).collect()).collect(),
        revision: 0,
    }
}

#[test]
fn copying_a_stale_selection_copies_what_is_left_not_a_panic() {
    let mut seed = 0x853c_49e6_748f_ea9b_u64;
    for _ in 0..2000 {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let source = source((seed % 5) as usize, ((seed >> 4) % 6) as usize);
        let (a, b) = ((seed >> 8) % 9, (seed >> 12) % 9);
        let (c, d) = (((seed >> 16) % 8) as usize, ((seed >> 20) % 8) as usize);
        let text = copied(&source, a..b, c..d, seed & 1 == 1);
        let width = d.min(source.columns.len()).saturating_sub(c);
        for line in text.lines() {
            assert!(line.is_empty() || line.split('\t').count() == width, "{line:?} {a}..{b} {c}..{d}");
        }
    }
    let shown = source(2, 2);
    assert_eq!(copied(&shown, 1..5, 1..4, true), "c1\n1.1");
    assert_eq!(copied(&shown, 3..5, 3..4, true), "");
}
