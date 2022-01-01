use std::{
    error::Error,
    io::Write,
    net::{TcpListener, ToSocketAddrs},
    sync::{Arc, Mutex},
    thread,
};

use crossbeam_channel::{bounded, Receiver, SendError, Sender, TrySelectError, TrySendError};

struct Frame {
    header: Vec<u8>,
    body: Vec<u8>,
}

impl Frame {
    fn from_jpeg_buf(buf: Vec<u8>) -> Self {
        Self {
            header: format!(
                "\r\n--MJPEGBOUNDARY\r\nContent-Length: {}\r\nX-Timestamp: 0.000000\r\n\r\n",
                buf.len()
            )
            .into_bytes(),
            body: buf,
        }
    }
}

pub struct MJpeg {
    send: Sender<Frame>,
    recv: Arc<Mutex<Receiver<Frame>>>,
}

impl MJpeg {
    /// 创建一个mjpeg推流器
    /// # example
    /// ```
    /// let m = Arc::new(MJpeg::new());
    /// ```
    pub fn new() -> Self {
        let (send, recv) = bounded(1);
        let recv = Arc::new(Mutex::new(recv));
        Self { send, recv }
    }

    /// 将流推送到mjpeg
    /// # example
    /// ```
    /// let m = Arc::new(MJpeg::new());
    /// let mrc = m.clone();
    /// thread::spawn(move || mrc.run("0.0.0.0:8088").unwrap());
    /// loop {
    ///     let b = camera.take_one().unwrap();
    ///     m.update_jpeg(b).unwrap();
    /// }
    /// ```
    // FIXME: convert this error into our own type (or the one from std),
    // to avoid exposing our dependency on crossbeam channel.
    pub fn update_jpeg(&self, buf: Vec<u8>) -> Result<(), SendError<Vec<u8>>> {
        self.send
            .send(Frame::from_jpeg_buf(buf))
            .map_err(|e| SendError(e.0.body))
    }

    /// 将流推送到mjpeg
    /// # example
    /// ```
    /// let m = Arc::new(MJpeg::new());
    /// let mrc = m.clone();
    /// thread::spawn(move || mrc.run("0.0.0.0:8088").unwrap());
    /// loop {
    ///     let b = camera.take_one().unwrap();
    ///     match m.try_update_jpeg(b) {
    ///         Ok(_) => (),
    ///         Err(TrySendError::Full(_b)) => println!("nobody is listening, or queue is backed up")
    ///         Err(TrySendError::Disconnected(_b)) => {
    ///             println!("disconnected");
    ///             break;
    ///         }
    ///     }
    /// }
    /// ```
    // FIXME: convert this error into our own type (or the one from std),
    // to avoid exposing our dependency on crossbeam channel.
    pub fn try_update_jpeg(&self, buf: Vec<u8>) -> Result<(), TrySendError<Vec<u8>>> {
        self.send
            .try_send(Frame::from_jpeg_buf(buf))
            .map_err(|e| match e {
                TrySendError::Disconnected(frame) => TrySendError::Disconnected(frame.body),
                TrySendError::Full(frame) => TrySendError::Full(frame.body),
            })
    }

    /// Ask whether the jpeg queue is full (happens when the reader disconnects or is slow to respond)
    pub fn is_full(&self) -> bool {
        self.send.is_full()
    }

    /// 设置mjpeg服务端口
    /// # example
    /// ```
    /// let m = Arc::new(MJpeg::new());
    /// let mrc = m.clone();
    /// // 此mjpeg-server将运行在8088端口
    /// thread::spawn(move || mrc.run("0.0.0.0:8088").unwrap());
    /// loop {
    ///     let b = camera.take_one().unwrap();
    ///     m.update_jpeg(b).unwrap();
    /// }
    /// ```
    pub fn run<A: ToSocketAddrs>(
        &self,
        addr: A,
    ) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
        let server = TcpListener::bind(addr)?;
        for stream in server.incoming() {
            let recv = self.recv.clone();
            thread::spawn(move || match stream {
                Ok(stream) => {
                    let mut stream = stream;
                    stream.write(b"HTTP/1.1 200 OK\r\nContent-Type: multipart/x-mixed-replace;boundary=MJPEGBOUNDARY\r\n").unwrap();
                    stream.flush().unwrap();
                    loop {
                        match recv.lock().map(|buf| buf.recv()) {
                            Ok(frame) => match frame {
                                Ok(mut frame) => {
                                    stream.write(&frame.header).unwrap();
                                    stream.write(&frame.body).unwrap();
                                    stream.flush().unwrap();
                                }
                                Err(e) => {
                                    println!("recv err{}", e)
                                }
                            },
                            Err(e) => {
                                println!("lock err{}", e)
                            }
                        };
                    }
                }
                Err(e) => {
                    println!("stream err{}", e)
                }
            });
        }
        Ok(())
    }
}
