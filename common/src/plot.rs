use plotters::prelude::*;
use plotters::style::{BLACK, WHITE};
use plotters::style::full_palette::*;

#[derive(Debug, Clone)]
pub struct Data {
  pub x: i64,
  pub y: f64,
}

pub struct Plot;

impl Plot {
  pub fn plot(series: Vec<Vec<Data>>, out_file: &str, title: &str, y_label: &str) -> anyhow::Result<()> {
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
    root.fill(&WHITE).map_err(
      |e| anyhow::anyhow!("Failed to fill drawing area with white: {}", e)
    )?;
    let mut chart = ChartBuilder::on(&root)
      .margin_top(20)
      .margin_bottom(20)
      .margin_left(30)
      .margin_right(30)
      .set_all_label_area_size(120)
      .caption(
        title,
        ("sans-serif", 40.0).into_font(),
      )
      .build_cartesian_2d(min_x..max_x, min_y..max_y).map_err(
      |e| anyhow::anyhow!("Failed to build cartesian 2d: {}", e)
    )?;
    chart
      .configure_mesh()
      .light_line_style(WHITE)
      .label_style(("sans-serif", 30, &BLACK).into_text_style(&root))
      .x_desc("UNIX Milliseconds")
      .y_desc(y_label)
      .draw().map_err(
      |e| anyhow::anyhow!("Failed to draw mesh: {}", e)
    )?;

    for data in series {
      let color = Self::random_color();
      chart.draw_series(
        LineSeries::new(
          data.iter().map(|data| (data.x, data.y)),
          ShapeStyle {
            color,
            filled: true,
            stroke_width: 2,
          },
        )
          .point_size(3),
      ).map_err(
        |e| anyhow::anyhow!("Failed to draw series: {}", e)
      )?;
    }

    root.present().map_err(
      |e| anyhow::anyhow!("Failed to present root: {}", e)
    )?;

    Ok(())
  }

  pub fn random_color() -> RGBAColor {
    let colors = [
      RED_A400,
      BLUEGREY_700,
      GREY_400,
      GREY_900,
      BROWN_700,
      DEEPORANGE_A200,
      DEEPORANGE_200,
      ORANGE_A200,
      AMBER_300,
      AMBER_800,
      YELLOW_600,
      LIME_400,
      LIME_800,
      LIGHTGREEN_700,
      GREEN_500,
      GREEN_900,
      TEAL_700,
      TEAL_200,
      CYAN_800,
      LIGHTBLUE_A200,
      BLUE_A700,
      BLUE_400,
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
    // get random color
    RGBAColor::from(colors[rand::random::<usize>() % colors.len()])
  }
}