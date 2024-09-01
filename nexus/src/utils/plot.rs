use crate::Data;
use plotters::prelude::*;
use plotters::style::full_palette::*;
use plotters::style::{BLACK, WHITE};

const FIRST: RGBColor = BLACK;
const SECOND: RGBColor = RED_A400;
const THIRD: RGBColor = GREEN_500;
const FOURTH: RGBColor = AMBER_800;
const FIFTH: RGBColor = BLUE_A700;
const SIXTH: RGBColor = PURPLE_A400;
const OTHER: RGBColor = GREY_400;

pub struct DualAxisConfig<'a> {
  pub out_file: &'a str,
  pub title: &'a str,
  pub x_label: &'a str,
  pub series: [Series; 2],
  pub y_label: &'a str,
  pub log_scale: Option<bool>,
  pub second_axis_series: [Series; 2],
  pub second_axis_y_label: &'a str,
  pub second_axis_log_scale: Option<bool>,
}

pub struct Series {
  pub data: Vec<Data>,
  pub label: String,
}

pub struct Plot;

impl Plot {
  pub fn plot(
    series: Vec<Series>,
    out_file: &str,
    title: &str,
    y_label: &str,
    x_label: &str,
    log_scale: Option<bool>,
  ) -> anyhow::Result<()> {
    let log_scale = log_scale.unwrap_or(true);
    let mut min_x = i64::MAX;
    let mut max_x = i64::MIN;
    let mut min_y = f64::MAX;
    let mut max_y = f64::MIN;
    for datum in series.iter().flat_map(|d| &d.data) {
      if datum.x < min_x {
        min_x = datum.x;
      }
      if datum.x > max_x {
        max_x = datum.x;
      }
      if datum.y < min_y {
        min_y = datum.y;
      }
      if datum.y > max_y {
        max_y = datum.y;
      }
    }

    let offset = 100.0;
    let to_log = |y: f64| (y + offset).log10();
    let from_log = |y: f64| 10f64.powf(y) - offset;

    let y_range = match log_scale {
      true => to_log(min_y)..to_log(max_y),
      false => min_y..max_y,
    };

    let y_label_formatter = |y: f64| match log_scale {
      true => format!("{:.2}", from_log(y)),
      false => format!("{:.2}", y),
    };

    let transform_y = |y: f64| match log_scale {
      true => to_log(y),
      false => y,
    };

    let root = BitMapBackend::new(out_file, (2048, 1024)).into_drawing_area();
    root.fill(&WHITE)?;
    let mut chart = ChartBuilder::on(&root)
      .set_all_label_area_size(150)
      .margin(20)
      .caption(title, ("sans-serif", 40.0).into_font())
      .build_cartesian_2d(min_x..max_x, y_range)?;

    chart
      .configure_mesh()
      .light_line_style(WHITE)
      .label_style(("sans-serif", 30, &BLACK).into_text_style(&root))
      .x_desc(x_label)
      .y_desc(y_label)
      .y_labels(10)
      .y_label_formatter(&|y| y_label_formatter(*y))
      .draw()?;

    let mut draw_series = |s: &Series, color: RGBColor| {
      chart
        .draw_series(
          LineSeries::new(
            s.data.iter().map(|data| (data.x, transform_y(data.y))),
            ShapeStyle {
              color: RGBAColor::from(color),
              filled: true,
              stroke_width: 1,
            },
          )
          .point_size(1),
        )
        .map_err(|e| anyhow::anyhow!("Failed to draw series: {}", e))?
        .label(s.label.as_str())
        .legend(move |(x, y)| {
          PathElement::new(
            [(x + 10, y + 1), (x, y)],
            ShapeStyle {
              color: RGBAColor::from(color),
              filled: true,
              stroke_width: 10,
            },
          )
        });
      Result::<_, anyhow::Error>::Ok(())
    };

    for (i, s) in series.iter().enumerate() {
      match i {
        0 => {
          draw_series(s, FIRST)?;
        }
        1 => {
          draw_series(s, SECOND)?;
        }
        2 => {
          draw_series(s, THIRD)?;
        }
        3 => {
          draw_series(s, FOURTH)?;
        }
        4 => {
          draw_series(s, FIFTH)?;
        }
        5 => {
          draw_series(s, SIXTH)?;
        }
        _ => {
          draw_series(s, OTHER)?;
        }
      }
    }

    chart
      .configure_series_labels()
      .position(SeriesLabelPosition::UpperLeft)
      .margin(20)
      .legend_area_size(30)
      .border_style(BLACK)
      .background_style(BLACK.mix(0.1))
      .label_font(("sans-serif", 24))
      .draw()
      .map_err(|e| anyhow::anyhow!("Failed to configure series labels: {}", e))?;

    root
      .present()
      .map_err(|e| anyhow::anyhow!("Failed to present root: {}", e))?;

    Ok(())
  }

  pub fn plot_dual_axis(cfg: DualAxisConfig<'_>) -> anyhow::Result<()> {
    let DualAxisConfig {
      out_file,
      title,
      x_label,
      series,
      y_label,
      log_scale,
      second_axis_series,
      second_axis_y_label,
      second_axis_log_scale,
    } = cfg;
    let log_scale = log_scale.unwrap_or(true);
    let mut min_x = i64::MAX;
    let mut max_x = i64::MIN;
    let mut min_y = f64::MAX;
    let mut max_y = f64::MIN;
    for datum in series.iter().flat_map(|d| &d.data) {
      if datum.x < min_x {
        min_x = datum.x;
      }
      if datum.x > max_x {
        max_x = datum.x;
      }
      if datum.y < min_y {
        min_y = datum.y;
      }
      if datum.y > max_y {
        max_y = datum.y;
      }
    }

    let second_axis_log_scale = second_axis_log_scale.unwrap_or(true);
    let mut second_axis_min_x = i64::MAX;
    let mut second_axis_max_x = i64::MIN;
    let mut second_axis_min_y = f64::MAX;
    let mut second_axis_max_y = f64::MIN;
    for datum in second_axis_series.iter().flat_map(|d| &d.data) {
      if datum.x < second_axis_min_x {
        second_axis_min_x = datum.x;
      }
      if datum.x > second_axis_max_x {
        second_axis_max_x = datum.x;
      }
      if datum.y < second_axis_min_y {
        second_axis_min_y = datum.y;
      }
      if datum.y > second_axis_max_y {
        second_axis_max_y = datum.y;
      }
    }

    let offset = 100.0;
    let to_log = |y: f64| (y + offset).log10();
    let from_log = |y: f64| 10f64.powf(y) - offset;

    let y_range = match log_scale {
      true => to_log(min_y)..to_log(max_y),
      false => min_y..max_y,
    };

    let second_axis_y_range = match log_scale {
      true => to_log(second_axis_min_y)..to_log(second_axis_max_y),
      false => second_axis_min_y..second_axis_max_y,
    };

    let y_label_formatter = |y: f64| match log_scale {
      true => format!("{:.2}", from_log(y)),
      false => format!("{:.2}", y),
    };
    let transform_y = |y: f64| match log_scale {
      true => to_log(y),
      false => y,
    };
    let transform_second_axis_y = |y: f64| match second_axis_log_scale {
      true => to_log(y),
      false => y,
    };

    let root = BitMapBackend::new(out_file, (2048, 1024)).into_drawing_area();
    root.fill(&WHITE)?;
    let mut chart = ChartBuilder::on(&root)
      .set_all_label_area_size(150)
      .margin(20)
      .caption(title, ("sans-serif", 40.0).into_font())
      .build_cartesian_2d(min_x..max_x, y_range)?
      .set_secondary_coord(second_axis_min_x..second_axis_max_x, second_axis_y_range);

    chart
      .configure_mesh()
      .light_line_style(WHITE)
      .label_style(("sans-serif", 30, &BLACK).into_text_style(&root))
      .x_desc(x_label)
      .y_desc(y_label)
      .y_labels(10)
      .y_label_formatter(&|y| y_label_formatter(*y))
      .draw()?;

    chart
      .configure_secondary_axes()
      .label_style(("sans-serif", 30, &BLACK).into_text_style(&root))
      .y_desc(second_axis_y_label)
      .draw()?;

    let mut draw_series = |s: &Series, color: RGBColor| {
      chart
        .draw_series(
          LineSeries::new(
            s.data.iter().map(|data| (data.x, transform_y(data.y))),
            ShapeStyle {
              color: RGBAColor::from(color),
              filled: true,
              stroke_width: 1,
            },
          )
          .point_size(1),
        )
        .map_err(|e| anyhow::anyhow!("Failed to draw series: {}", e))?
        .label(s.label.as_str())
        .legend(move |(x, y)| {
          PathElement::new(
            [(x + 10, y + 1), (x, y)],
            ShapeStyle {
              color: RGBAColor::from(color),
              filled: true,
              stroke_width: 10,
            },
          )
        });
      Result::<_, anyhow::Error>::Ok(())
    };
    for (i, s) in series.iter().enumerate() {
      match i {
        0 => {
          draw_series(s, FIRST)?;
        }
        1 => {
          draw_series(s, SECOND)?;
        }
        _ => {
          return Err(anyhow::anyhow!("Too many series"));
        }
      }
    }

    let mut draw_second_series = |s: &Series, color: RGBColor| {
      chart
        .draw_secondary_series(
          LineSeries::new(
            s.data
              .iter()
              .map(|data| (data.x, transform_second_axis_y(data.y))),
            ShapeStyle {
              color: RGBAColor::from(color),
              filled: true,
              stroke_width: 1,
            },
          )
          .point_size(1),
        )
        .map_err(|e| anyhow::anyhow!("Failed to draw series: {}", e))?
        .label(s.label.as_str())
        .legend(move |(x, y)| {
          PathElement::new(
            [(x + 10, y + 1), (x, y)],
            ShapeStyle {
              color: RGBAColor::from(color),
              filled: true,
              stroke_width: 10,
            },
          )
        });
      Result::<_, anyhow::Error>::Ok(())
    };
    for (i, s) in second_axis_series.iter().enumerate() {
      match i {
        0 => {
          draw_second_series(s, THIRD)?;
        }
        1 => {
          draw_second_series(s, FOURTH)?;
        }
        _ => {
          return Err(anyhow::anyhow!("Too many series"));
        }
      }
    }

    chart
      .configure_series_labels()
      .position(SeriesLabelPosition::UpperLeft)
      .margin(20)
      .legend_area_size(30)
      .border_style(BLACK)
      .background_style(BLACK.mix(0.1))
      .label_font(("sans-serif", 24))
      .draw()
      .map_err(|e| anyhow::anyhow!("Failed to configure series labels: {}", e))?;

    root
      .present()
      .map_err(|e| anyhow::anyhow!("Failed to present root: {}", e))?;

    Ok(())
  }

  pub fn plot_without_legend(
    series: Vec<Vec<Data>>,
    out_file: &str,
    title: &str,
    y_label: &str,
    x_label: &str,
  ) -> anyhow::Result<()> {
    let all: Vec<&Data> = series.iter().flatten().collect();

    let mut min_x = i64::MAX;
    let mut max_x = i64::MIN;
    let mut min_y = f64::MAX;
    let mut max_y = f64::MIN;
    for datum in all.iter() {
      if datum.x < min_x {
        min_x = datum.x;
      }
      if datum.x > max_x {
        max_x = datum.x;
      }
      if datum.y < min_y {
        min_y = datum.y;
      }
      if datum.y > max_y {
        max_y = datum.y;
      }
    }

    let root = BitMapBackend::new(out_file, (2048, 1024)).into_drawing_area();
    root
      .fill(&WHITE)
      .map_err(|e| anyhow::anyhow!("Failed to fill drawing area with white: {}", e))?;
    let mut chart = ChartBuilder::on(&root)
      .margin_top(20)
      .margin_bottom(20)
      .margin_left(30)
      .margin_right(30)
      .set_all_label_area_size(170)
      .caption(title, ("sans-serif", 40.0).into_font())
      .build_cartesian_2d(min_x..max_x, min_y..max_y)
      .map_err(|e| anyhow::anyhow!("Failed to build cartesian 2d: {}", e))?;
    chart
      .configure_mesh()
      .light_line_style(WHITE)
      .label_style(("sans-serif", 30, &BLACK).into_text_style(&root))
      .x_desc(x_label)
      .y_desc(y_label)
      .draw()
      .map_err(|e| anyhow::anyhow!("Failed to draw mesh: {}", e))?;

    let mut draw_series = |data: &[Data], color: RGBColor| {
      chart
        .draw_series(
          LineSeries::new(
            data.iter().map(|data| (data.x, data.y)),
            ShapeStyle {
              color: RGBAColor::from(color),
              filled: true,
              stroke_width: 1,
            },
          )
          .point_size(1),
        )
        .map_err(|e| anyhow::anyhow!("Failed to draw series: {}", e))?;
      Result::<_, anyhow::Error>::Ok(())
    };

    for (i, data) in series.iter().enumerate() {
      match i {
        0 => {
          draw_series(data.as_slice(), FIRST)?;
        }
        1 => {
          draw_series(data.as_slice(), SECOND)?;
        }
        2 => {
          draw_series(data.as_slice(), THIRD)?;
        }
        3 => {
          draw_series(data.as_slice(), FOURTH)?;
        }
        4 => {
          draw_series(data.as_slice(), FIFTH)?;
        }
        5 => {
          draw_series(data.as_slice(), SIXTH)?;
        }
        _ => {
          draw_series(data.as_slice(), OTHER)?;
        }
      }
    }

    root
      .present()
      .map_err(|e| anyhow::anyhow!("Failed to present root: {}", e))?;

    Ok(())
  }

  pub fn red() -> RGBColor {
    RED_A400
  }

  pub fn blue() -> RGBColor {
    BLUEGREY_700
  }

  pub fn random_color() -> RGBColor {
    let colors = [
      RED_A400,
      BLUEGREY_700,
      GREY_400,
      GREY_900,
      BROWN_700,
      DEEPORANGE_A200,
      DEEPORANGE_200,
      ORANGE_A200,
      AMBER_800,
      YELLOW_600,
      LIGHTGREEN_700,
      GREEN_500,
      TEAL_700,
      CYAN_800,
      LIGHTBLUE_A200,
      BLUE_A700,
      BLUE_800,
      INDIGO_800,
      INDIGO_300,
      DEEPPURPLE_A100,
      PURPLE_A400,
      PURPLE_200,
      PINK_600,
      RED_800,
      RED_200,
    ];
    colors[rand::random::<usize>() % colors.len()]
  }
}
