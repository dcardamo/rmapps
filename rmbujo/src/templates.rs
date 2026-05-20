//! askama template structs (compile-time checked). Each renders an HTML fragment.

use askama::Template;

#[derive(Clone, Debug)]
pub struct DayView {
    pub day: u32,
    pub weekday: &'static str,
    pub week_start: bool,
}

#[derive(Template)]
#[template(path = "base.html")]
pub struct Base<'a> {
    pub css: &'a str,
    pub pages: &'a [String],
}

#[derive(Template)]
#[template(path = "cover.html")]
pub struct Cover<'a> {
    pub year: i32,
    pub title: &'a str,
    pub blank_title: bool,
}

#[derive(Template)]
#[template(path = "dotgrid.html")]
pub struct DotGrid;

#[derive(Template)]
#[template(path = "tasks.html")]
pub struct Tasks;

#[derive(Template)]
#[template(path = "month_index.html")]
pub struct MonthIndex<'a> {
    pub month_name: &'a str,
    pub year: i32,
    pub days: &'a [DayView],
}

#[derive(Template)]
#[template(path = "future_log.html")]
pub struct FutureLog<'a> {
    pub months: &'a [&'a str],
}

#[derive(Template)]
#[template(path = "reference.html")]
pub struct Reference;
