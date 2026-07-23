//! gRPC 兼容状态码。

/// 所有 qRPC/gRPC 兼容状态码。
pub const ALL_ERROR_CODES: [ErrorCode; 17] = [
    ErrorCode::Ok,
    ErrorCode::Cancelled,
    ErrorCode::Unknown,
    ErrorCode::InvalidArgument,
    ErrorCode::DeadlineExceeded,
    ErrorCode::NotFound,
    ErrorCode::AlreadyExists,
    ErrorCode::PermissionDenied,
    ErrorCode::ResourceExhausted,
    ErrorCode::FailedPrecondition,
    ErrorCode::Aborted,
    ErrorCode::OutOfRange,
    ErrorCode::Unimplemented,
    ErrorCode::Internal,
    ErrorCode::Unavailable,
    ErrorCode::DataLoss,
    ErrorCode::Unauthenticated,
];

/// gRPC 数值兼容状态码。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ErrorCode {
    Ok = 0,
    Cancelled = 1,
    Unknown = 2,
    InvalidArgument = 3,
    DeadlineExceeded = 4,
    NotFound = 5,
    AlreadyExists = 6,
    PermissionDenied = 7,
    ResourceExhausted = 8,
    FailedPrecondition = 9,
    Aborted = 10,
    OutOfRange = 11,
    Unimplemented = 12,
    Internal = 13,
    Unavailable = 14,
    DataLoss = 15,
    Unauthenticated = 16,
}

impl ErrorCode {
    pub const fn as_u8(self) -> u8 {
        self as u8
    }
}

impl TryFrom<u64> for ErrorCode {
    type Error = u64;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Ok),
            1 => Ok(Self::Cancelled),
            2 => Ok(Self::Unknown),
            3 => Ok(Self::InvalidArgument),
            4 => Ok(Self::DeadlineExceeded),
            5 => Ok(Self::NotFound),
            6 => Ok(Self::AlreadyExists),
            7 => Ok(Self::PermissionDenied),
            8 => Ok(Self::ResourceExhausted),
            9 => Ok(Self::FailedPrecondition),
            10 => Ok(Self::Aborted),
            11 => Ok(Self::OutOfRange),
            12 => Ok(Self::Unimplemented),
            13 => Ok(Self::Internal),
            14 => Ok(Self::Unavailable),
            15 => Ok(Self::DataLoss),
            16 => Ok(Self::Unauthenticated),
            value => Err(value),
        }
    }
}
