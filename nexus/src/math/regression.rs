use crate::{Data, Dataset};
use nalgebra::{dvector, DMatrix, DVector};
use varpro::prelude::*;
use varpro::solvers::levmar::{LevMarProblemBuilder, LevMarSolver};

/// A function that computes the coefficients of a quadratic least squares regression
/// for a set of stock prices.
///
/// # Arguments
///
/// * `x` - A vector of x values (time or index of the stock prices).
/// * `y` - A vector of y values (stock prices).
///
/// # Returns
///
/// * A tuple `(a, b, c)` representing the coefficients of the quadratic polynomial
///   `y = ax^2 + bx + c`.
fn quadratic_least_squares(x: &[i64], y: &[f64]) -> (f64, f64, f64) {
  let n = x.len();

  if n != y.len() || n < 3 {
    panic!("The input vectors must have the same length and contain at least 3 points.");
  }

  // Create the design matrix for a quadratic fit
  let mut design_matrix = DMatrix::zeros(n, 3);
  for i in 0..n {
    design_matrix[(i, 0)] = (x[i] * x[i]) as f64;
    design_matrix[(i, 1)] = x[i] as f64;
    design_matrix[(i, 2)] = 1.0;
  }

  // Convert y into a DVector
  let y_vector = DVector::from_column_slice(y);

  // Perform the least squares fitting
  let coefficients = (design_matrix.transpose() * &design_matrix)
    .try_inverse()
    .expect("Matrix is singular and cannot be inverted")
    * design_matrix.transpose()
    * y_vector;

  // Extract coefficients
  let a = coefficients[0];
  let b = coefficients[1];
  let c = coefficients[2];

  (a, b, c)
}

/// A function to predict the next `n` data points using a quadratic least squares regression.
///
/// # Arguments
///
/// * `a`, `b`, `c` - The coefficients of the quadratic polynomial.
/// * `start_x` - The x value from which to start predicting (one past the last known x).
/// * `n` - The number of future points to predict.
///
/// # Returns
///
/// * A vector of predicted y values for the next `n` points.
pub fn quad_lsr_extrap(data: Dataset, extrapolate: usize, extrap_only: bool) -> Dataset {
  let (a, b, c) = quadratic_least_squares(data.x().as_slice(), data.y().as_slice());

  let mut in_sample = Vec::new();
  for i in 0..data.len() {
    let x = data.0[i].x as f64;
    let y = a * x * x + b * x + c;
    in_sample.push(Data { x: data.0[i].x, y });
  }

  let start_x = data.0[data.0.len() - 1].x + 1;
  let mut predictions = Vec::new();
  for i in 0..extrapolate {
    let x = (start_x + i as i64) as f64;
    let y = a * x * x + b * x + c;
    predictions.push(y);
  }
  let extrapolation: Vec<Data> = predictions
    .into_iter()
    .enumerate()
    .map(|(i, y)| Data {
      x: (start_x + i as i64),
      y,
    })
    .collect();

  match extrap_only {
    true => Dataset::new(extrapolation),
    false => {
      let full_data = in_sample.into_iter().chain(extrapolation).collect();
      Dataset::new(full_data)
    }
  }
}

/// A function that computes the coefficients of a cubic least squares regression
/// for a set of stock prices.
///
/// # Arguments
///
/// * `x` - A vector of x values (time or index of the stock prices).
/// * `y` - A vector of y values (stock prices).
///
/// # Returns
///
/// * A tuple `(a, b, c, d)` representing the coefficients of the cubic polynomial
///   `y = ax^3 + bx^2 + cx + d`.
fn cubic_least_squares(x: &[i64], y: &[f64]) -> (f64, f64, f64, f64) {
  let n = x.len();

  if n != y.len() || n < 4 {
    panic!("The input vectors must have the same length and contain at least 4 points.");
  }

  // Create the design matrix for a cubic fit
  let mut design_matrix = DMatrix::zeros(n, 4);
  for i in 0..n {
    design_matrix[(i, 0)] = (x[i] * x[i] * x[i]) as f64;
    design_matrix[(i, 1)] = (x[i] * x[i]) as f64;
    design_matrix[(i, 2)] = x[i] as f64;
    design_matrix[(i, 3)] = 1.0;
  }

  // Convert y into a DVector
  let y_vector = DVector::from_column_slice(y);

  // Perform the least squares fitting
  let coefficients = (design_matrix.transpose() * &design_matrix)
    .try_inverse()
    .expect("Matrix is singular and cannot be inverted")
    * design_matrix.transpose()
    * y_vector;

  // Extract coefficients
  let a = coefficients[0];
  let b = coefficients[1];
  let c = coefficients[2];
  let d = coefficients[3];

  (a, b, c, d)
}

/// A function to predict the next `n` data points using a quadratic least squares regression.
///
/// # Arguments
///
/// * `a`, `b`, `c` - The coefficients of the quadratic polynomial.
/// * `start_x` - The x value from which to start predicting (one past the last known x).
/// * `n` - The number of future points to predict.
///
/// # Returns
///
/// * A vector of predicted y values for the next `n` points.
pub fn cubic_lsr_extrap(data: Dataset, extrapolate: usize, extrap_only: bool) -> Dataset {
  let (a, b, c, d) = cubic_least_squares(data.x().as_slice(), data.y().as_slice());

  let mut in_sample = Vec::new();
  for i in 0..data.len() {
    let x = data.0[i].x as f64;
    let y = a * x.powi(3) + b * x.powi(2) + c * x + d;
    in_sample.push(Data { x: data.0[i].x, y });
  }

  let start_x = data.0[data.0.len() - 1].x + 1;
  let mut predictions = Vec::new();
  for i in 0..extrapolate {
    let x = (start_x + i as i64) as f64;
    let y = a * x.powi(3) + b * x.powi(2) + c * x + d;
    predictions.push(y);
  }
  let extrapolation: Vec<Data> = predictions
    .into_iter()
    .enumerate()
    .map(|(i, y)| Data {
      x: (start_x + i as i64),
      y,
    })
    .collect();
  if extrap_only {
    Dataset::new(extrapolation)
  } else {
    let full_data = in_sample.into_iter().chain(extrapolation).collect();
    Dataset::new(full_data)
  }
}

// Define the exponential decay e^(-t/tau).
// Both of the nonlinear basis functions in this example
// are exponential decays.
fn exp_decay(t: &DVector<f64>, tau: f64) -> DVector<f64> {
  t.map(|t| (-t / tau).exp())
}

// the partial derivative of the exponential
// decay with respect to the nonlinear parameter tau.
// d/dtau e^(-t/tau) = e^(-t/tau)*t/tau^2
fn exp_decay_dtau(t: &DVector<f64>, tau: f64) -> DVector<f64> {
  t.map(|t| (-t / tau).exp() * t / tau.powi(2))
}

pub fn varpro_lsr_extrap(data: Dataset, extrapolate: usize, extrap_only: bool) -> Dataset {
  // temporal (or spatial) coordinates of the observations (x-axis)
  let x = data.x().into_iter().map(|x| x as f64).collect::<Vec<f64>>();
  let t: DVector<f64> = DVector::from_iterator(x.len(), x);
  // the observations we want to fit (y-axis)
  let y: DVector<f64> = DVector::from_iterator(data.len(), data.y());

  // 1. create the model by giving only the nonlinear parameter names it depends on
  let model = SeparableModelBuilder::<f64>::new(&["tau1", "tau2"])
    // provide the nonlinear basis functions and their derivatives.
    // In general, base functions can depend on more than just one parameter.
    // first function:
    .function(&["tau1"], exp_decay)
    .partial_deriv("tau1", exp_decay_dtau)
    // second function and derivatives with respect to all parameters
    // that it depends on (just one in this case)
    .function(&["tau2"], exp_decay)
    .partial_deriv("tau2", exp_decay_dtau)
    // a constant offset is added as an invariant base function
    // as a vector of ones. It is multiplied with its own linear coefficient,
    // creating a fittable offset
    .invariant_function(|v| DVector::from_element(v.len(), 1.))
    // give the coordinates of the problem
    .independent_variable(t)
    // provide guesses only for the nonlinear parameters in the
    // order that they were given on construction.
    .initial_parameters(vec![2.5, 5.5])
    .build()
    .unwrap();
  // 2. Cast the fitting problem as a nonlinear least squares minimization problem
  let problem = LevMarProblemBuilder::new(model)
    .observations(y)
    .build()
    .unwrap();
  // 3. Solve the fitting problem
  let fit_result = LevMarSolver::default()
    .fit(problem)
    .expect("fit must exit successfully");
  // 4. obtain the nonlinear parameters after fitting
  let alpha = fit_result.nonlinear_parameters();
  // 5. obtain the linear parameters
  let c = fit_result.linear_coefficients().unwrap();

  let mut in_sample = Vec::new();
  for i in 0..data.len() {
    let x = data.0[i].x as f64;
    let y = c[0] * exp_decay(&dvector![x], alpha[0])[0]
      + c[1] * exp_decay(&dvector![x], alpha[1])[0]
      + c[2];
    in_sample.push(Data { x: data.0[i].x, y });
  }

  // start after the last known x value and extrapolate
  let start_x = data.x().last().unwrap() + 1;
  let mut predictions = Vec::new();
  for i in 0..extrapolate {
    let x = (start_x + i as i64) as f64;
    let y = c[0] * exp_decay(&dvector![x], alpha[0])[0]
      + c[1] * exp_decay(&dvector![x], alpha[1])[0]
      + c[2];
    predictions.push(y);
  }

  let extrapolation: Vec<Data> = predictions
    .into_iter()
    .enumerate()
    .map(|(i, y)| Data {
      x: (start_x + i as i64),
      y,
    })
    .collect();

  match extrap_only {
    true => Dataset::new(extrapolation),
    false => {
      let full_data = in_sample.into_iter().chain(extrapolation).collect();
      Dataset::new(full_data)
    }
  }
}
