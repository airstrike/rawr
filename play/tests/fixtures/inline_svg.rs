let circle = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">
  <circle cx="50" cy="50" r="40" fill="hotpink"/>
</svg>"#;

svg(svg::Handle::from_memory(circle.as_bytes())).width(160).height(160)
