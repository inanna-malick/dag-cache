use headless_chrome::{protocol::page::ScreenshotFormat, Browser, LaunchOptionsBuilder, Tab};
use std::io::Write;

use std::sync::Arc;

// navigate to fresh page, make modification to head node, wait for save, take screenshot
fn stage_1(b: &Browser) {
    let tab = b.wait_for_initial_tab().unwrap();
    tab.navigate_to("http://localhost:3030")
        .expect("navigate-to");

    tab.wait_for_element(".node-header")
        .unwrap()
        .click()
        .unwrap();
    tab.type_str("test header")
        .unwrap()
        .press_key("Enter")
        .unwrap();

    // TODO: would be ideal to not actually create node until edit op finishes..
    tab.wait_for_element(".add-sub-node")
        .unwrap()
        .click()
        .unwrap();
    tab.type_str("test subheader")
        .unwrap()
        .press_key("Enter")
        .unwrap();

    screenshot("notes-app-stage-0.png", tab.clone());

    // trigger save
    tab.wait_for_element(".trigger-save")
        .unwrap()
        .click()
        .unwrap();

    tab.wait_for_element(".state-is-unmodified").unwrap();

    screenshot("notes-app-stage-1.png", tab);
}

fn screenshot(s: &str, tab: Arc<Tab>) {
    // Take a screenshot of the entire browser window
    let png = tab
        .capture_screenshot(ScreenshotFormat::PNG, None, true)
        .unwrap();

    let mut file = std::fs::File::create(s).unwrap();
    file.write_all(&png).unwrap();
}

// navigate to modified page, expand via load node, take screenshot
fn stage_2(b: &Browser) {
    let tab = b.wait_for_initial_tab().unwrap();
    tab.navigate_to("http://localhost:3030")
        .expect("navigate-to");

    // load node then wait for header to materialize
    tab.wait_for_element(".load-node").unwrap().click().unwrap();
    tab.wait_for_element(".node-header").unwrap();

    screenshot("notes-app-stage-2.png", tab);
}

fn main() {
    let opt = LaunchOptionsBuilder::default()
        .headless(true)
        .build()
        .unwrap();
    let b = Browser::new(opt).unwrap();
    println!("init test");
    stage_1(&b);
    println!("stage 1 done");
    stage_2(&b);
    println!("stage 2 done");
}
