//! PyO3 module for qRPC Python binding.

mod async_bridge;

use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use pyo3::types::PyBytes;
use qrpc::Status;

#[pyfunction]
fn unary_roundtrip<'py>(
    py: Python<'py>,
    payload: &Bound<'_, PyBytes>,
) -> PyResult<Bound<'py, PyBytes>> {
    let response =
        async_bridge::unary_roundtrip(payload.as_bytes().to_vec()).map_err(status_to_pyerr)?;
    Ok(PyBytes::new(py, &response))
}

#[pymodule]
fn _qrpc(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(unary_roundtrip, m)?)?;
    Ok(())
}

fn status_to_pyerr(status: Status) -> PyErr {
    PyRuntimeError::new_err(format!("{:?}: {}", status.code, status.message))
}
