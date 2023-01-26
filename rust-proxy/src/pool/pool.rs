use async_trait::async_trait;
use log::{debug, error, info, Level};
use std::error::Error;
use std::fmt;
use tokio::net::TcpStream;

#[derive(Debug)]
pub struct MyError {
    details: String,
}

impl MyError {
    fn new(msg: &str) -> MyError {
        MyError {
            details: msg.to_string(),
        }
    }
}

impl fmt::Display for MyError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.details)
    }
}

impl Error for MyError {
    fn description(&self) -> &str {
        &self.details
    }
}
#[derive(Clone, Debug)]
pub struct TcpConnectionManager {
    backendUrl: String,
}

impl TcpConnectionManager {
    pub fn new(info: String) -> Result<TcpConnectionManager, MyError> {
        Ok(TcpConnectionManager { backendUrl: info })
    }
}

#[async_trait]
impl bb8::ManageConnection for TcpConnectionManager {
    type Connection = TcpStream;
    type Error = MyError;

    async fn connect(&self) -> Result<Self::Connection, Self::Error> {
        debug!("start connect");
        return match TcpStream::connect(self.backendUrl.clone()).await {
            Ok(tcpStream) => Ok(tcpStream),
            Err(err) => 
            {
                error!("connect error,error is{}",err);
                Err(MyError::new(err.to_string().as_str()))},
        };
    }

    async fn is_valid(&self, conn: &mut Self::Connection) -> Result<(), Self::Error> {
        let mut b1 = [0; 1];
        debug!("peek start");
        conn.peek(&mut b1).await;
        debug!("peek successfully");
        return Ok(());
    }

     fn has_broken(&self, conn: &mut Self::Connection) -> bool{
        debug!("has_broken start");
       return false;
    }
}
