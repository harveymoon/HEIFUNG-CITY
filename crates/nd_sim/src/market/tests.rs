
/// [inspector] The books keep a bounded month, oldest first.
#[test]
fn books_keep_the_last_month() {
    let mut b = Business::new(BusinessKind::Grocery, 4);
    for d in 0..(Business::BOOK_DAYS + 5) {
        b.close_books(d as f32, 1.0);
    }
    assert_eq!(b.books.len(), Business::BOOK_DAYS);
    assert_eq!(b.books[0].revenue, 5.0);
    assert_eq!(b.books.last().unwrap().revenue, (Business::BOOK_DAYS + 4) as f32);
}

use super::*;

#[test]
fn hours_wrap_midnight_and_split_into_shifts() {
    let venue = BusinessKind::Venue.hours();
    assert!(venue.contains(23) && venue.contains(2) && !venue.contains(3) && !venue.contains(12));
    assert_eq!(venue.span(), 9);
    assert_eq!(venue.shift(1), venue, "a short day is one shift");
    let plant = BusinessKind::Mill.hours();
    assert_eq!(plant.shift(0), Hours::new(6, 14));
    assert_eq!(plant.shift(1), Hours::new(14, 22));
    let park = BusinessKind::Parking.hours();
    assert!((0..24).all(|h| park.contains(h)));
    let shifts: Vec<Hours> = (0..3).map(|s| park.shift(s)).collect();
    assert_eq!(shifts, vec![Hours::new(0, 8), Hours::new(8, 16), Hours::new(16, 0)]);
    // Every hour of a long day is covered by some shift.
    let groc = BusinessKind::Grocery.hours();
    for h in 0..24 {
        assert_eq!(groc.contains(h), (0..4).any(|s| groc.shift(s).contains(h)), "hour {h}");
    }
}
