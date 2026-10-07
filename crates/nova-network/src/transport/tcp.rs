//! Gerçek TCP transport katmanı — tokio async TCP + Noise XX + FrameCodec
//!
//! ## Mimari
//!
//! ```text
//! TcpTransport::bind("0.0.0.0:30333")
//!     └── TcpListener (tokio)
//!
//! TcpTransport::dial("peer:30333", remote_key)
//!     └── TcpStream::connect()
//!         └── Noise XX el sıkışma (3 mesaj)
//!             └── TcpPeer { stream, codec: FrameCodec }
//!
//! TcpTransport::accept()
//!     └── TcpListener::accept()
//!         └── Noise XX el sıkışma (yanıtlayıcı)
//!             └── TcpPeer { stream, codec: FrameCodec }
//! ```
//!
//! ## Mesaj framing
//!
//! Her mesaj length-prefixed:
//! ```text
//! [length: u32 BE][frame_codec_encoded_payload: length bytes]
//! ```
//! Bu format üstüne FrameCodec'in kendi nonce+MAC katmanı eklenir.
//!
//! ## Polkadot referansı
//! `sc-network/src/transport.rs` → `build_transport()` pattern:
//! TCP → WsTransport → Noise → Yamux → framing
//!
//! ## Avalanchego referansı
//! `network/network.go` → `peer.Conn` → `gorilla/websocket` veya `net.Conn`
//! üzerinden `codec.Manager` framing

use super::{FrameCodec, NoiseSession};
use crate::errors::{NetworkError, NetworkResult};
use crate::transport::noise::NoiseMessage;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};

// ─── Framing sabitleri ────────────────────────────────────────────────────────

/// Maksimum mesaj boyutu (4 MiB) — DoS koruması
const MAX_MESSAGE_SIZE: u32 = 4 * 1024 * 1024;

// ─── TcpPeer ──────────────────────────────────────────────────────────────────

/// Kurulmuş TCP bağlantısı — Noise XX tamamlandı, FrameCodec aktif
///
/// Mesajlar FrameCodec üzerinden şifreli ve MAC'li gönderilir.
pub struct TcpPeer {
    stream: TcpStream,
    codec: FrameCodec,
    /// Uzak peer'in Noise statik anahtarı (identity)
    pub remote_key: [u8; 32],
    /// Uzak adres (IP:port)
    pub remote_addr: String,
}

impl TcpPeer {
    /// Şifreli mesaj gönder
    ///
    /// Format: `[length: u32 BE][FrameCodec encoded payload]`
    pub async fn send(&mut self, payload: &[u8]) -> NetworkResult<()> {
        let frame = self.codec.encode_frame(payload);
        let len = frame.len() as u32;
        if len > MAX_MESSAGE_SIZE {
            return Err(NetworkError::ProtocolError {
                reason: format!("mesaj çok büyük: {} > {}", len, MAX_MESSAGE_SIZE),
            });
        }
        self.stream.write_all(&len.to_be_bytes()).await
            .map_err(|e| NetworkError::IoError { reason: e.to_string() })?;
        self.stream.write_all(&frame).await
            .map_err(|e| NetworkError::IoError { reason: e.to_string() })?;
        Ok(())
    }

    /// Şifreli mesaj al (blocking — bir sonraki mesajı bekle)
    pub async fn recv(&mut self) -> NetworkResult<Vec<u8>> {
        // Uzunluk öneki oku
        let mut len_buf = [0u8; 4];
        self.stream.read_exact(&mut len_buf).await
            .map_err(|e| NetworkError::IoError { reason: e.to_string() })?;
        let len = u32::from_be_bytes(len_buf);

        if len == 0 || len > MAX_MESSAGE_SIZE {
            return Err(NetworkError::ProtocolError {
                reason: format!("geçersiz mesaj uzunluğu: {}", len),
            });
        }

        // Frame verisi oku
        let mut frame = vec![0u8; len as usize];
        self.stream.read_exact(&mut frame).await
            .map_err(|e| NetworkError::IoError { reason: e.to_string() })?;

        // FrameCodec ile decode
        self.codec.decode_frame(&frame)
    }

    /// Bağlantıyı kapat
    pub async fn close(mut self) -> NetworkResult<()> {
        self.stream.shutdown().await
            .map_err(|e| NetworkError::IoError { reason: e.to_string() })
    }

    pub fn remote_key(&self) -> [u8; 32] {
        self.remote_key
    }

    pub fn remote_addr(&self) -> &str {
        &self.remote_addr
    }

    /// TCP stream'i okuma/yazma yarılarına böl — mutex'siz bidirectional iletişim için.
    ///
    /// `TcpPeerWriter` → `tcp_peers` listesinde tutulur (broadcast için).
    /// `TcpPeerReader` → ayrı tokio task'ında tutulur (mutex tutmadan recv için).
    pub fn into_split(self) -> (TcpPeerWriter, TcpPeerReader) {
        let (read_half, write_half) = self.stream.into_split();
        let (encoder_codec, decoder_codec) = self.codec.into_split();
        (
            TcpPeerWriter { write: write_half, codec: encoder_codec, remote_key: self.remote_key, remote_addr: self.remote_addr },
            TcpPeerReader { read: read_half, codec: decoder_codec, remote_key: self.remote_key },
        )
    }
}

// ─── TcpPeerWriter ────────────────────────────────────────────────────────────

/// TCP bağlantısının yazma yarısı — sadece gönderme, mutex'siz recv mümkün.
pub struct TcpPeerWriter {
    write: OwnedWriteHalf,
    codec: FrameCodec,
    pub remote_key: [u8; 32],
    pub remote_addr: String,
}

impl TcpPeerWriter {
    pub async fn send(&mut self, payload: &[u8]) -> NetworkResult<()> {
        let frame = self.codec.encode_frame(payload);
        let len = frame.len() as u32;
        if len > MAX_MESSAGE_SIZE {
            return Err(NetworkError::ProtocolError {
                reason: format!("mesaj çok büyük: {} > {}", len, MAX_MESSAGE_SIZE),
            });
        }
        self.write.write_all(&len.to_be_bytes()).await
            .map_err(|e| NetworkError::IoError { reason: e.to_string() })?;
        self.write.write_all(&frame).await
            .map_err(|e| NetworkError::IoError { reason: e.to_string() })?;
        Ok(())
    }
}

// ─── TcpPeerReader ────────────────────────────────────────────────────────────

/// TCP bağlantısının okuma yarısı — ayrı task'ta, `tcp_peers` mutex'ini tutmadan.
pub struct TcpPeerReader {
    read: OwnedReadHalf,
    codec: FrameCodec,
    pub remote_key: [u8; 32],
}

impl TcpPeerReader {
    pub async fn recv(&mut self) -> NetworkResult<Vec<u8>> {
        let mut len_buf = [0u8; 4];
        self.read.read_exact(&mut len_buf).await
            .map_err(|e| NetworkError::IoError { reason: e.to_string() })?;
        let len = u32::from_be_bytes(len_buf);
        if len == 0 || len > MAX_MESSAGE_SIZE {
            return Err(NetworkError::ProtocolError {
                reason: format!("geçersiz mesaj uzunluğu: {}", len),
            });
        }
        let mut frame = vec![0u8; len as usize];
        self.read.read_exact(&mut frame).await
            .map_err(|e| NetworkError::IoError { reason: e.to_string() })?;
        self.codec.decode_frame(&frame)
    }
}

// ─── TcpTransport ─────────────────────────────────────────────────────────────

/// Gerçek TCP transport — dinleyici + dial + Noise XX
///
/// Polkadot `build_transport()` ve Avalanchego `network.go` analogu.
pub struct TcpTransport {
    local_key: [u8; 32],
    listener: Option<TcpListener>,
    /// Dinleme adresi
    pub listen_addr: Option<String>,
}

impl TcpTransport {
    /// Yeni transport oluştur (henüz bind edilmemiş)
    pub fn new(local_key: [u8; 32]) -> Self {
        Self { local_key, listener: None, listen_addr: None }
    }

    /// Belirli adreste TCP dinleyici başlat
    ///
    /// Port 0 kullanılırsa işletim sistemi uygun bir port seçer.
    pub async fn bind(local_key: [u8; 32], addr: &str) -> NetworkResult<Self> {
        let listener = TcpListener::bind(addr).await
            .map_err(|e| NetworkError::IoError { reason: format!("bind hatası {addr}: {e}") })?;
        let listen_addr = listener.local_addr()
            .map(|a| a.to_string())
            .ok();
        Ok(Self { local_key, listener: Some(listener), listen_addr })
    }

    /// Uzak peer'a bağlan (Noise XX başlatıcı)
    ///
    /// 1. TCP bağlantı kur
    /// 2. Noise XX: msg1 gönder, msg2 al, msg3 gönder
    /// 3. TcpPeer döndür
    pub async fn dial(&self, addr: &str) -> NetworkResult<TcpPeer> {
        let stream = TcpStream::connect(addr).await
            .map_err(|e| NetworkError::IoError { reason: format!("dial hatası {addr}: {e}") })?;
        let remote_addr = stream.peer_addr().map(|a| a.to_string()).unwrap_or_default();

        let (peer, _stream) = Self::handshake_initiator(stream, self.local_key).await?;
        let _ = _stream;
        Ok(TcpPeer {
            stream: peer.0,
            codec: peer.1,
            remote_key: peer.2,
            remote_addr,
        })
    }

    /// Gelen bağlantı kabul et (Noise XX yanıtlayıcı)
    ///
    /// `listener` ile bind edilmiş olması gerekir.
    pub async fn accept(&self) -> NetworkResult<TcpPeer> {
        let listener = self.listener.as_ref().ok_or(NetworkError::IoError {
            reason: "transport bind edilmedi".into(),
        })?;
        let (stream, peer_addr) = listener.accept().await
            .map_err(|e| NetworkError::IoError { reason: format!("accept hatası: {e}") })?;
        let remote_addr = peer_addr.to_string();

        let (peer, _stream) = Self::handshake_responder(stream, self.local_key).await?;
        let _ = _stream;
        Ok(TcpPeer {
            stream: peer.0,
            codec: peer.1,
            remote_key: peer.2,
            remote_addr,
        })
    }

    /// Yerel dinleme adresini döndür (bind sonrası)
    pub fn local_addr(&self) -> Option<&str> {
        self.listen_addr.as_deref()
    }

    // ─── Noise XX Yardımcıları ────────────────────────────────────────────────

    /// Başlatıcı el sıkışması: msg1 gönder → msg2 al → msg3 gönder
    async fn handshake_initiator(
        mut stream: TcpStream,
        local_key: [u8; 32],
    ) -> NetworkResult<((TcpStream, FrameCodec, [u8; 32]), ())> {
        let mut initiator = NoiseSession::new_initiator(local_key);

        // msg1 gönder
        let msg1 = initiator.write_message_1()?;
        Self::send_noise_msg(&mut stream, &msg1).await?;

        // msg2 al
        let msg2 = Self::recv_noise_msg(&mut stream).await?;
        let msg3 = initiator.process_message_2_and_finalize(&msg2)?;

        // msg3 gönder
        Self::send_noise_msg(&mut stream, &msg3).await?;

        if !initiator.is_complete() {
            return Err(NetworkError::HandshakeFailed { reason: "Noise XX tamamlanamadı".into() });
        }

        let remote_key = initiator.remote_public_key()
            .ok_or(NetworkError::HandshakeFailed { reason: "remote key alınamadı".into() })?;
        let session_key = initiator.session_key()
            .ok_or(NetworkError::HandshakeFailed { reason: "session key türetilemedi".into() })?;

        Ok(((stream, FrameCodec::new(session_key), remote_key), ()))
    }

    /// Yanıtlayıcı el sıkışması: msg1 al → msg2 gönder → msg3 al
    async fn handshake_responder(
        mut stream: TcpStream,
        local_key: [u8; 32],
    ) -> NetworkResult<((TcpStream, FrameCodec, [u8; 32]), ())> {
        let mut responder = NoiseSession::new_responder(local_key);

        // msg1 al
        let msg1 = Self::recv_noise_msg(&mut stream).await?;
        let msg2 = responder.process_message_1_and_respond(&msg1)?;

        // msg2 gönder
        Self::send_noise_msg(&mut stream, &msg2).await?;

        // msg3 al
        let msg3 = Self::recv_noise_msg(&mut stream).await?;
        responder.process_message_3(&msg3)?;

        if !responder.is_complete() {
            return Err(NetworkError::HandshakeFailed { reason: "Noise XX tamamlanamadı".into() });
        }

        let remote_key = responder.remote_public_key()
            .ok_or(NetworkError::HandshakeFailed { reason: "remote key alınamadı".into() })?;
        let session_key = responder.session_key()
            .ok_or(NetworkError::HandshakeFailed { reason: "session key türetilemedi".into() })?;

        Ok(((stream, FrameCodec::new(session_key), remote_key), ()))
    }

    /// NoiseMessage'ı length-prefixed JSON olarak TCP stream'e yaz
    async fn send_noise_msg(stream: &mut TcpStream, msg: &NoiseMessage) -> NetworkResult<()> {
        let bytes = serde_json::to_vec(msg)
            .map_err(|e| NetworkError::ProtocolError { reason: format!("Noise serializasyon: {e}") })?;
        let len = bytes.len() as u32;
        stream.write_all(&len.to_be_bytes()).await
            .map_err(|e| NetworkError::IoError { reason: e.to_string() })?;
        stream.write_all(&bytes).await
            .map_err(|e| NetworkError::IoError { reason: e.to_string() })?;
        Ok(())
    }

    /// Length-prefixed JSON'dan NoiseMessage oku
    async fn recv_noise_msg(stream: &mut TcpStream) -> NetworkResult<NoiseMessage> {
        let mut len_buf = [0u8; 4];
        stream.read_exact(&mut len_buf).await
            .map_err(|e| NetworkError::IoError { reason: e.to_string() })?;
        let len = u32::from_be_bytes(len_buf) as usize;

        if len == 0 || len > 4096 {
            return Err(NetworkError::ProtocolError {
                reason: format!("geçersiz Noise mesaj uzunluğu: {}", len),
            });
        }

        let mut buf = vec![0u8; len];
        stream.read_exact(&mut buf).await
            .map_err(|e| NetworkError::IoError { reason: e.to_string() })?;

        serde_json::from_slice(&buf)
            .map_err(|e| NetworkError::ProtocolError { reason: format!("Noise deserialization: {e}") })
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use x25519_dalek::{PublicKey as X25519Public, StaticSecret};

    fn local_key(n: u8) -> [u8; 32] {
        [n; 32]
    }

    fn x25519_pub(seed: [u8; 32]) -> [u8; 32] {
        *X25519Public::from(&StaticSecret::from(seed)).as_bytes()
    }

    /// Gerçek TCP bağlantısı: bind → dial → mesaj alışverişi
    #[tokio::test]
    async fn test_tcp_dial_accept_handshake() {
        // Sunucu: rastgele port (0 = işletim sistemi seçer)
        let server = TcpTransport::bind(local_key(0xAA), "127.0.0.1:0").await.unwrap();
        let server_addr = server.local_addr().unwrap().to_string();

        // İstemci ve sunucu eş zamanlı çalışır
        let client = TcpTransport::new(local_key(0xBB));
        let (server_peer, client_peer) = tokio::join!(
            server.accept(),
            client.dial(&server_addr)
        );

        let server_peer = server_peer.unwrap();
        let client_peer = client_peer.unwrap();

        // Her iki taraf da karşılıklı X25519 public key'leri doğru bulmalı
        assert_eq!(server_peer.remote_key(), x25519_pub(local_key(0xBB)));
        assert_eq!(client_peer.remote_key(), x25519_pub(local_key(0xAA)));
    }

    /// Mesaj gönder/al — şifreli iletişim testi
    #[tokio::test]
    async fn test_tcp_send_recv() {
        let server = TcpTransport::bind(local_key(1), "127.0.0.1:0").await.unwrap();
        let addr = server.local_addr().unwrap().to_string();

        let client = TcpTransport::new(local_key(2));
        let (server_peer_res, client_peer_res) = tokio::join!(
            server.accept(),
            client.dial(&addr)
        );

        let mut server_peer = server_peer_res.unwrap();
        let mut client_peer = client_peer_res.unwrap();

        // İstemci → Sunucu
        let msg = b"Nova Shield TCP mesaji";
        client_peer.send(msg).await.unwrap();
        let received = server_peer.recv().await.unwrap();
        assert_eq!(received, msg);
    }

    /// Çift yönlü mesaj alışverişi
    #[tokio::test]
    async fn test_tcp_bidirectional() {
        let server = TcpTransport::bind(local_key(3), "127.0.0.1:0").await.unwrap();
        let addr = server.local_addr().unwrap().to_string();

        let client = TcpTransport::new(local_key(4));
        let (server_peer_res, client_peer_res) = tokio::join!(
            server.accept(),
            client.dial(&addr)
        );

        let mut s = server_peer_res.unwrap();
        let mut c = client_peer_res.unwrap();

        // İstemci → Sunucu
        c.send(b"ping").await.unwrap();
        let pong_payload = s.recv().await.unwrap();
        assert_eq!(pong_payload, b"ping");

        // Sunucu → İstemci
        s.send(b"pong").await.unwrap();
        let ping_reply = c.recv().await.unwrap();
        assert_eq!(ping_reply, b"pong");
    }

    /// Çoklu mesaj — sıralı iletim testi (FrameCodec nonce monoton artar)
    #[tokio::test]
    async fn test_tcp_multiple_messages_sequential() {
        let server = TcpTransport::bind(local_key(5), "127.0.0.1:0").await.unwrap();
        let addr = server.local_addr().unwrap().to_string();

        let client = TcpTransport::new(local_key(6));
        let (server_peer_res, client_peer_res) = tokio::join!(
            server.accept(),
            client.dial(&addr)
        );

        let mut s = server_peer_res.unwrap();
        let mut c = client_peer_res.unwrap();

        for i in 0u8..10 {
            let msg = vec![i; 100];
            c.send(&msg).await.unwrap();
            let recv = s.recv().await.unwrap();
            assert_eq!(recv, msg, "mesaj {} bozulmuş", i);
        }
    }

    /// Büyük mesaj (128 KiB) gönderimi
    #[tokio::test]
    async fn test_tcp_large_message() {
        let server = TcpTransport::bind(local_key(7), "127.0.0.1:0").await.unwrap();
        let addr = server.local_addr().unwrap().to_string();

        let client = TcpTransport::new(local_key(8));
        let (server_peer_res, client_peer_res) = tokio::join!(
            server.accept(),
            client.dial(&addr)
        );

        let mut s = server_peer_res.unwrap();
        let mut c = client_peer_res.unwrap();

        let large = vec![0xFFu8; 128 * 1024]; // 128 KiB
        c.send(&large).await.unwrap();
        let recv = s.recv().await.unwrap();
        assert_eq!(recv.len(), large.len());
    }

    /// Bağlantısız dial → hata döner
    #[tokio::test]
    async fn test_tcp_dial_no_server_fails() {
        // Port 1 dinleme gerektiren yetki — bağlantı başarısız olmalı
        let result = TcpTransport::new(local_key(9)).dial("127.0.0.1:1").await;
        assert!(result.is_err(), "Sunucu olmadan dial başarısız olmalı");
    }

    /// Bind addr kontrolü
    #[tokio::test]
    async fn test_tcp_transport_bind_local_addr() {
        let transport = TcpTransport::bind(local_key(0xCC), "127.0.0.1:0").await.unwrap();
        let addr = transport.local_addr().unwrap();
        assert!(addr.starts_with("127.0.0.1:"), "Adres 127.0.0.1 ile başlamalı: {}", addr);
        // Port 0 değil, işletim sistemi atadığı port olmalı
        let port_str = addr.split(':').last().unwrap();
        let port: u16 = port_str.parse().unwrap();
        assert!(port > 0, "İşletim sistemi pozitif port atamalı");
    }
}
