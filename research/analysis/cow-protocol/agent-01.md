# CoW Protocol Analiz — Agent 01
## Kapsam: model/src/ — Order Modeli (11 dosya)
## DEX Ozel Protokolu: 3x Hassasiyet

---

## Genel Bakis

CoW Protocol'un `model` crate'i, orderbook ile solver arasinda paylasilan veri modellerini tanimlar.
Bu crate bir API katmani degildir; sadece serializasyon/deserializasyon modelleri icerir.
Tum tipler `serde` ile JSON uyumlu; EIP-712 imzalama, OrderUid hesaplama, fee politikalari bu
crate'te modellenir. "Colocated" ve "non-colocated" solver mimarisi buradaki modeller uzerinde calisir.

Toplam analiz edilen kaynak: ~1800 satir (11 dosya).

---

## Dosya Analizi

---

### [lib.rs] (~235 satirlik)

**Amac:** Tum alt modulleri re-export eder; `TokenPair`, `DomainSeparator`, `AuctionId` gibi
paylasilmis primitive tipleri tanimlar.

**Struct/Enum/Trait Listesi:**

- `TokenPair` (satir 25): Iki ERC-20 token adresini siralanmis sekilde tutan tuple struct.
  - `0: Address` her zaman kucuk olan adres, `1: Address` her zaman buyuk olan adres.
  - `new(a, b) -> Option<Self>`: Eger `a == b` ise `None` doner (ayni token cifti gecersiz).
    `a < b` ise `TokenPair(a, b)`, `a > b` ise `TokenPair(b, a)`. Siralama deterministik.
  - `contains(&self, token) -> bool`: Token ciftin icinde mi kontrolu.
  - `other(&self, token) -> Option<Address>`: Ciftteki diger tokeni dondurur.
  - `get(&self) -> (Address, Address)`: Her zaman `(kucuk, buyuk)` sirasiyla doner.
  - `first_ord() -> Self`: `(0x00..00, 0x00..01)` — `Ord` trait'i icin en kucuk deger.
  - `IntoIterator` impl: Hem `TokenPair` hem `&TokenPair` icin `chain` iterator.

- `DomainSeparator` (satir 95): `[u8; 32]` newtype, EIP-712 domain separator.
  - `new(chain_id: u64, contract_address: Address) -> Self`:
    `Eip712Domain { name: "Gnosis Protocol", version: "v2", chain_id, verifying_contract }` ile
    `alloy_sol_types::Eip712Domain::separator()` cagrilarak olusturulur.
  - Sepolia test: `chain_id=11155111`, adres `0x9008D19f58AAbD9eD0D60971565AA8510560ab41` icin
    `0xdaee378bd0eb30ddf479272accf91761e697bc00e067a268f95f1d2732ed230b` bekleniyor. Test passes.
  - `FromStr` impl: Hex string'den parse edilebilir.
  - `Debug` impl: Hex string olarak yazdirir.

- `AuctionId` (satir 21): `type AuctionId = i64` — her auction'in benzersiz ID'si.

**Matematiksel Formuller:**

- EIP-712 Domain Separator:
  ```
  domainSeparator = keccak256(
    keccak256("EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)")
    || keccak256("Gnosis Protocol")
    || keccak256("v2")
    || uint256(chain_id)
    || address(contract_address)
  )
  ```

**Guvenlik Analizi:**

- `TokenPair::new` token adreslerinin ayni olup olmadigini kontrol eder. Ayni token ile cift
  olusturmak imkansiz — bu AMM havuzu mantigi icin kritik bir guarddir. Yoksa kendi kendine
  takas saldirisi mumkun olabilirdi.
- `DomainSeparator` chain_id ve contract adresini icerir: baska bir chain veya baska bir kontrat
  uzerinde ayni imzanin gecerli olmasi (cross-chain replay) engellenmiş olur.

**DEX Uygulama Notu — Nova Shield icin:**

- Nova Shield'in kendi `DomainSeparator` implementasyonu icin ayni yapi kullanilabilir: chain_id +
  settlement_contract_address. Bu, replay saldirilarini engellemek icin zorunludur.
- `TokenPair` yapisi, Nova Shield'in liquidity pool dizini icin aynen kullanilabilir. Siralama
  deterministik oldugu icin hashmap key olarak kullanmak guvenlidir.
- Nasil daha iyi yapabiliriz: `AuctionId`'yi `i64` degil `u64` yapabiliriz — negatif auction ID
  anlamsiz; bu daha katı tip güvenligi saglar. CoW negatif deger kullanmiyor ama i64 secimleri
  veritabani uyumlulugundandir (PostgreSQL BIGINT = i64).

---

### [order.rs] (~1300+ satirlik — EN KRITIK DOSYA)

**Amac:** CoW Protocol'un temel order tipini, OrderUid'yi, siniflandirmayi ve validity kontrollerini tanimlar.

**Struct/Enum/Trait Listesi:**

- `BUY_ETH_ADDRESS` (satir 34): `Address::repeat_byte(0xee)` = `0xEEEe...EEEe`.
  Native token (ETH veya chain'in yerel tokeni) satin alimlarinda gercek ERC-20 adresi olmadigi
  icin bu "magic address" kullanilir. Solver bu adresi gorurse ETH transferi yapar.

- `Interactions` (satir 37):
  - `pre: Vec<InteractionData>` — settlement'tan once calistirilan on-chain islemler
  - `post: Vec<InteractionData>` — settlement'tan sonra calistirilan on-chain islemler

- `Order` (satir 48): Orderbook'tan donen tam order.
  - `metadata: OrderMetadata` — orderbook tarafindan eklenen metadata (flatten)
  - `data: OrderData` — imzalanan ve kontrat tarafindan dogrulanan core data (flatten)
  - `signature: Signature` — kullanicinin imzasi (flatten)
  - `interactions: Interactions` — kullanicinin pre/post hook'lari
  - `contains_token_from(&self, token_list) -> bool`: sell veya buy token whitelist'te mi?
  - `is_user_order(&self) -> bool`: Market veya Limit ise true, Liquidity ise false.
  - `is_limit_order(&self) -> bool`: sadece Limit sinifi ise true.

- `OrderStatus` (satir 61):
  - `PresignaturePending` — PreSign mekanizmasiyla imzalanacak, on-chain konfirmasyon bekleniyor
  - `Open` (default) — aktif, settler bekleniyor
  - `Fulfilled` — basariyla gerceklestirildi
  - `Cancelled` — kullanici iptal etti
  - `Expired` — `valid_to` gecti

- `OrderBuilder` (satir 87): Builder pattern, test ve entegrasyon icin kullanilir.
  Tum `with_*` metodlari chaining yapiyor. `sign_with()` metodu hem `owner`'i hem `uid`'yi
  hem de `signature`'u ayni anda set eder — tutarlilik saglar.

- `OrderData` (satir 201): EIP-712 ile imzalanan ve kontrat tarafindan dogrulanan core alanlar.
  - `sell_token: Address` — satilan ERC-20 token adresi
  - `buy_token: Address` — satin alinan ERC-20 token adresi (veya BUY_ETH_ADDRESS)
  - `receiver: Option<Address>` — alici; `None` ise owner'a gonder
  - `sell_amount: U256` — MAKSIMUM satilebilecek miktar
  - `buy_amount: U256` — MINIMUM satin alinacak miktar
  - `valid_to: u32` — Unix timestamp (saniye), bu blok'tan sonra order gecersiz
  - `app_data: AppDataHash` — [u8; 32], kullanici metadata hash'i
  - `fee_amount: U256` — DEPRECATED; 0 olmali
  - `kind: OrderKind` — Buy veya Sell
  - `partially_fillable: bool` — kismen doldurulabilir mi?
  - `sell_token_balance: SellTokenSource` — token kaynagi
  - `buy_token_balance: BuyTokenDestination` — token hedefi

  `TYPE_HASH` (satir 229):
  ```
  0xd5a25ba2e97094ad7d83dc28a6572da797d6b3e7fc6663bd93efb789fc17e489
  ```
  GPv2Order Solidity struct'indan `keccak256` ile uretilmistir. Kontrat ile byte-by-byte eslesmelidir.

  `hash_struct(&self) -> [u8; 32]` (satir 236): EIP-712 `hashStruct` implementasyonu.
  416 byte'lik buffer hazirlanir, her alan 32-byte slot'a yazilir:
  ```
  [0..32]   = TYPE_HASH
  [44..64]  = sell_token (20 bytes, 12 byte padding)
  [76..96]  = buy_token
  [108..128]= receiver (yoksa Address::ZERO)
  [128..160]= sell_amount (big-endian U256)
  [160..192]= buy_amount
  [220..224]= valid_to (big-endian u32, 28 byte padding)
  [224..256]= app_data
  [256..288]= fee_amount
  [288..320]= kind hash
  [351]     = partially_fillable (1 byte, 31 byte padding)
  [352..384]= sell_token_balance hash
  [384..416]= buy_token_balance hash
  result    = keccak256(buf[0..416])
  ```

  `token_pair(&self) -> Option<TokenPair>`: sell ve buy token'dan `TokenPair` olusturur.

  `uid(&self, domain, owner) -> OrderUid`: EIP-712 mesajini hesaplar ve 56-byte UID uretir.

  `within_market(&self, quote: QuoteAmounts) -> bool` (satir 272):
  Market order validasyonu. U512 widening mul ile overflow oneliyor.
  ```
  (sell_amount + fee_amount) * quote.buy >= (quote.sell + quote.fee) * buy_amount
  ```

- `QuoteAmounts` (satir 283): `sell: U256, buy: U256, fee: U256`.

- `OrderCreation` (satir 293): Frontend'den POST /orders endpoint'ine gelen order.
  OrderData alanlari + ek:
  - `from: Option<Address>` — EthFlow durumunda EthFlow kontrat adresi
  - `signature: Signature` (flatten)
  - `quote_id: Option<QuoteId>`
  - `app_data: OrderCreationAppData` (flatten)
  - `full_balance_check: bool`

  `data(&self) -> OrderData`: app_data hash'ini resolve ederek `OrderData` dondurur.

  `sign(self, scheme, domain, key) -> Self`: ECDSA ile imzalar.

  `verify_owner(&self, domain, app_data_signer) -> Result<Address, VerificationError>` (satir 397):
  Sahip dogrulamasi — en karmasik metot:
  1. `signature.recover(domain, data().hash_struct())` ile ECDSA recovery
  2. `from` ve `app_data_signer`'i birlestir (coalesce): ikisi de varsa esit olmali
  3. Recovered imzaci ile beklenen `from`'u karsilastir
  4. Durum matrisi:
     - `(Some(from), Some(recovered))` ve `from == recovered.signer`: OK
     - `(Some(from), None)` (on-chain imzalar): OK, `from` kullanilir
     - `(None, Some(recovered))`: OK, recovered signer kullanilir
     - `(Some(_), Some(recovered))` ve eslesmez: `UnexpectedSigner` hatasi
     - `(None, None)`: `MissingFrom` hatasi

- `OrderCreationAppData` (satir 441): Untagged enum:
  - `Both { full: String, expected: AppDataHash }` — tam JSON + beklenen hash
  - `Hash { hash: AppDataHash }` — sadece hash (eski format)
  - `Full { full: String }` — sadece tam JSON, hash hesaplanir
  `hash()`: `Full`/`Both` icin `hash_full_app_data()`, `Hash` icin dogrudan deger.

- `VerificationError` (satir 496):
  - `UnableToRecoverSigner(anyhow::Error)`
  - `UnexpectedSigner(signature::Recovered)`
  - `MissingFrom`
  - `AppdataFromMismatch(AppdataFromMismatch)`

- `OrderCancellations` (satir 504): Toplu iptal. `order_uids: Vec<OrderUid>`.
  TYPE_HASH: `keccak256("OrderCancellations(bytes[] orderUid)")` = `0x4c89ef...`
  `hash_struct`: Her UID keccak256'lanir -> array hash -> TYPE_HASH ile birlestirilir -> keccak256.

- `SignedOrderCancellations` (satir 532): Toplu iptal + ECDSA imzasi.
  `validate()` ile imzaci dogrulama.

- `OrderCancellation` (satir 556): Tekil iptal.
  TYPE_HASH: `keccak256("OrderCancellation(bytes orderUid)")` = `0x7b41b3...`
  `for_order()`, `hash_struct()`, `validate()` metodlari mevcut.

- `CancellationPayload` (satir 617): API iptal payload'i. `signature + signing_scheme`.

- `EthflowData` (satir 623): Native ETH order ek verisi.
  - `user_valid_to: i64`, `refund_tx_hash: Option<B256>`
  Custom `Serialize`: `is_refunded = refund_tx_hash.is_some()` derived field ekleniyor.

- `OnchainOrderPlacementError` (satir 659): 8 variant:
  `ValidToTooFarInTheFuture`, `DisabledOrderClass`, `PreValidationError`, `InvalidQuote`,
  `InsufficientFee`, `NonZeroFee`, `InvalidOrderData`, `Other`.

- `OnchainOrderData` (satir 683): `sender: Address, placement_error: Option<OnchainOrderPlacementError>`.

- `OrderMetadata` (satir 695): Orderbook'un ekledigi alanlar.
  - `creation_date: DateTime<Utc>`
  - `owner: Address`
  - `uid: OrderUid`
  - `available_balance: Option<U256>` (deprecated, null)
  - `executed_buy_amount: BigUint`
  - `executed_sell_amount: BigUint`
  - `executed_sell_amount_before_fees: U256`
  - `executed_fee_amount: U256`
  - `executed_fee: U256`
  - `executed_fee_token: Address`
  - `invalidated: bool`
  - `status: OrderStatus`
  - `class: OrderClass` (flatten, "class" JSON key)
  - `settlement_contract: Address`
  - `ethflow_data: Option<EthflowData>`
  - `onchain_order_data: Option<OnchainOrderData>`
  - `onchain_user: Option<Address>`
  - `is_liquidity_order: bool`
  - `full_app_data: Option<String>`
  - `quote: Option<OrderQuote>`

- `OrderUid` (satir 743): `[u8; 56]` newtype.
  **Yapi:**
  ```
  [0..32]  = EIP-712 order hash (hashed_eip712_message sonucu)
  [32..52] = owner address (20 bytes)
  [52..56] = valid_to (4 bytes, big-endian u32)
  ```
  Hex: 112 karakter + "0x" = 114 karakter (limit: 128).

  `from_parts(hash, owner, valid_to) -> Self`: 3 parcadan UID olusturur.
  `parts(&self) -> (B256, Address, u32)`: Parcalara ayirir.
  `from_integer(i: u32) -> Self`: Test kolayligi — ilk 4 byte little-endian.

- `OrderKind` (satir 858):
  - `Buy` (default): Kullanici X miktar token almak istiyor. `buy_amount` SABIT (minimum garanti),
    `sell_amount` MAKSIMUM (ust sinir). Solver en az `buy_amount` alip en fazla `sell_amount` harcar.
  - `Sell`: Kullanici X miktar token satmak istiyor. `sell_amount` SABIT (harcanan),
    `buy_amount` MINIMUM. Solver `sell_amount` alip en az `buy_amount` verir.

  Sabitler:
  - `BUY = keccak256("buy") = 0x6ed88e868af0a1983e3886d5f3e95a2fafbd6c3450bc229e27342283dc429ccc`
  - `SELL = keccak256("sell") = 0xf3b277728b3fee749481eb3e0b3b48980dbbab78658fc419025cb16eee346775`

  `from_contract_bytes(kind: [u8; 32]) -> Result<Self>`: Kontrat'tan okunan 32-byte'i cevirir.

- `OrderClass` (satir 886):
  - `Market` (default): Normal kullanici order'i, hemen fulfill edilmesi beklenir.
  - `Liquidity`: Sadece whitelist kullanicilarin koyabilecegi CoW (coincidence of wants) order'i.
    Sifir fee'li, hemen fulfill edilmesi beklenmiyor.
  - `Limit`: Belirli fiyata ulasinca fulfill edilecek order. Fee sifir, surplus'tan alinir.

  `is_limit() -> bool`.
  Serialize: `#[serde(tag = "class")]` ile `"class": "market"/"limit"/"liquidity"`.

- `SellTokenSource` (satir 946):
  - `Erc20` (default): Vault relayer'a ERC-20 approve + transfer.
  - `External`: Balancer Vault uzerinden ERC-20 transfer.
  - `Internal`: Balancer Vault internal balance'dan cekim.

  Sabitler: `keccak256("erc20")`, `keccak256("external")`, `keccak256("internal")`.
  `as_bytes()` ve `from_contract_bytes()` ile kontrat uyumlulugu.

- `BuyTokenDestination` (satir 1007):
  - `Erc20` (default): ERC-20 transfer.
  - `Internal`: Balancer Vault internal balance.

- `OrderQuote` (satir 1043): Order olusturuldugu quote referans verisi.
  `gas_amount, gas_price, sell_token_price` (BigDecimal), `sell_amount, buy_amount, fee_amount` (U256),
  `solver: Address, verified: bool, metadata: serde_json::Value`.

**Matematiksel Formuller:**

1. **OrderUid Turetimi:**
   ```
   eip712_hash = keccak256(0x19 || 0x01 || domain_separator[32] || hash_struct[32])
   order_uid   = eip712_hash[0..32] || owner[0..20] || valid_to.to_be_bytes()[0..4]
   ```

2. **hash_struct (416 byte buffer):** (yukarida detayli verilmistir)

3. **Market Order Validasyonu:**
   ```
   (sell_amount + fee_amount) * quote.buy >= (quote.sell + quote.fee) * buy_amount
   ```
   U512 widening mul: overflow imkansiz.

4. **Sell Order Surplus:**
   ```
   actual_buy > buy_amount ise surplus = actual_buy - buy_amount (solver/protocol paylaşır)
   ```

5. **Buy Order Surplus:**
   ```
   actual_sell < sell_amount ise surplus = sell_amount - actual_sell (solver/protocol paylaşır)
   ```

**Guvenlik Analizi:**

- **Replay Attack:** DomainSeparator icinde chain_id + contract_address. Cross-chain replay engellenir.
- **Front-running:** CoW batch auction modeli — order'lar ayni blokta batch settle edilir.
  Front-running window pratikte yok.
- **Timestamp Manipulasyonu:** `valid_to` u32. 2106'da overflow eder. Orderbook maksimum validity
  siniri uyguluyor (ValidToTooFarInTheFuture). Miner timestamp manipulasyonu: +-15 saniye mumkun
  ama orderlar genellikle dakikalar-saatler icin gecerli, bu etki minimal.
- **Amount Overflow:** U512 widening_mul kullanilmis, overflow oneliyor.
- **Sahte Imza:** `verify_owner` ECDSA recovery + `from` alanini dogrular. Eslesme yoksa hata.
- **fee_amount Deprecation:** Sifir olmayan `fee_amount` orderlar reddedilir (NonZeroFee).
- **EthFlow Magic:** `BUY_ETH_ADDRESS` (`0xEEEE...EE`) — sozlesme icinde ozel isleme tabii tutuluyor.
  Normal ERC-20 ile karistirilmamali. Bu "magic" yaklasim tip guvenligi acisindan zayif.

**DEX Uygulama Notu — Nova Shield icin:**

- **OrderUid**: 56-byte format (32 hash + 20 owner + 4 valid_to) zarif tasarim. Nova Shield da
  ayni formati kullanmali. valid_to'nun uid'e dahil edilmesi, ayni hash fakat farkli validity
  suresine sahip order'larin farkli UID almasini saglar.
- **OrderKind semantigi**: Sell/Buy ayrimi kritik. "Sell X, minimum Y al" ve "Buy Y, maksimum X ver"
  semantikleri AMM'den farkli — limit order mantigi. Her ikisi de desteklenmeli.
- **Partially Fillable**: AMM havuzlari icin gerekli degil; OTC ve limit order defteri icin zorunlu.
  Kismi doldurmada `executed_sell_amount` titiz sekilde takip edilmeli.
- **OrderClass**: Market/Limit/Liquidity ayrimi Nova Shield icin de gerekli. Liquidity orderlar
  "solver-internal" — kendi AMM havuzu pozisyonlarini temsil edebilir.
- **Nasil daha iyi yapabiliriz:**
  - `valid_to` u32 yerine u64 (Layer 1 icin uzun vade, ama EVM uyumlulugu gerektiriyorsa u32).
  - `BUY_ETH_ADDRESS` magic yerine explicit `NativeToken` enum — type safety.
  - `app_data` field'i icin full JSON depolanmasi Nova Shield'da daha zengin metadata saglar.

---

### [fee_policy.rs] (62 satirlik)

**Amac:** Protocol fee politikalarini tanimlar. Uc farkli fee hesaplama stratejisi.

**Struct/Enum/Trait Listesi:**

- `FeePolicy` (satir 12): Fee stratejisi enum'u.
  - `Surplus { factor: f64, max_volume_factor: f64 }`: Surplus'tan fee al.
    `factor`: Surplus'un yuzde kaci alinir (0.0 - 1.0).
    `max_volume_factor`: Hacim bazli maksimum sinir.
  - `Volume { factor: f64 }`: Hacim bazli fee.
    `factor`: Islem hacminin yuzde kaci.
  - `PriceImprovement { factor: f64, max_volume_factor: f64, quote: Quote }`: Fiyat iyilestirme fee'si.
    `factor`: Fiyat iyilestirmesinin yuzde kaci.
    `max_volume_factor`: Hacim bazli maksimum sinir.
    `quote`: Referans quote (beklenen fiyat).

  `max_volume_factor(&self) -> f64`:
  - `Surplus` -> `max_volume_factor`
  - `Volume` -> `factor` (ayni anlama geliyor; volume fee icin factor zaten max)
  - `PriceImprovement` -> `max_volume_factor`

- `Quote` (satir 43): Referans fiyat teklifi.
  - `sell_amount: U256`, `buy_amount: U256`, `fee: U256`
  PriceImprovement hesabinda "beklenen fiyat" olarak kullanilir.

- `ExecutedProtocolFee` (satir 56): Bir islemde gerceklesen protocol fee.
  - `policy: FeePolicy`
  - `amount: U256`
  - `token: Address`

**Matematiksel Formuller:**

1. **Surplus Fee:**
   ```
   // Sell order icin:
   surplus = actual_buy_amount - expected_buy_amount

   // Buy order icin:
   surplus = expected_sell_amount - actual_sell_amount

   fee_from_surplus = surplus * factor
   fee_from_volume  = trade_volume * max_volume_factor

   protocol_fee = min(fee_from_surplus, fee_from_volume)
   ```
   `factor` genellikle 0.5 (surplus'un %50'si).

2. **Volume Fee:**
   ```
   protocol_fee = trade_volume * factor
   ```
   trade_volume = sell order icin sell_amount, buy order icin buy_amount.

3. **Price Improvement Fee:**
   ```
   quote_price   = quote.buy_amount / (quote.sell_amount + quote.fee)
   actual_price  = actual_buy / actual_sell

   price_improvement          = actual_price - quote_price
   price_improvement_tokens   = price_improvement * actual_sell

   protocol_fee = min(price_improvement_tokens * factor, trade_volume * max_volume_factor)
   ```
   Kullanici quote'dan daha iyi fiyat elde ettiyse, o iyilestirmenin bir kismini protocol alir.

**Guvenlik Analizi:**

- `f64` kullanimi: Factor'lar `f64` — off-chain hesaplama icin kabul edilebilir. Ancak on-chain
  settlement'ta integer aritmetigi ile yeniden hesaplama zorunludur. Off-chain/on-chain uyumsuzluk
  potansiyel zayiflik: kucuk floating point farklari fee hesabini bozabilir.
- `max_volume_factor` siniri: Surplus buyuk olsa bile `min(surplus_fee, volume_fee)` ile sinirlanir.
  Kucuk hacimli buyuk surplus orderlarinin overcharge edilmesi onleniyor.

**DEX Uygulama Notu — Nova Shield icin:**

- Uc fee politikasi Nova Shield icin ideal baslangic noktasi.
  - Surplus fee: Batch auction ortaminda ideal, solver kalitesini odullendirir.
  - Volume fee: AMM benzeri ortamda basit ve tahmin edilebilir.
  - Price improvement fee: Limit order'lar icin ideal.
- `f64` yerine fixed-point aritmetik (U256 ile basis points) kullanmak daha guvenli.
  Nova Shield on-chain fee hesaplamak istiyorsa kesinlikle integer aritmetigi kullanmali.
- `max_volume_factor` kavrami Nova Shield'a alinmali — buyuk surpluslerin tamamini almak
  predatory fee yaratir.

---

### [interaction.rs] (29 satirlik)

**Amac:** On-chain interaction'lari (pre/post hook) modeller.

**Struct/Enum/Trait Listesi:**

- `InteractionData` (satir 12): Tek bir on-chain interaction.
  - `target: Address` — cagrilacak kontrat
  - `value: U256` — gonderilecek ETH miktari
  - `call_data: Vec<u8>` — ABI-encoded calldata

  `Debug` impl: `call_data`'yi hex string olarak yazdirir.

**Guvenlik Analizi:**

- `target` adresine `value` ETH + `call_data` ile arbitrary call. Bu cok guclu ama tehlikeli.
  Settlement kontrati pre-hook'larda sell token approve'u yapilabilir; post-hook'larda arbitraj.
  CoW'un settlement kontrati balans kontrolu yaparak kotu niyetli interactionlari onler.
- Arbitrary call yeteneginin simulation zorunlu kilinmasi (CoW bunu yapıyor) — aksi halde
  settlement kontratinin fund'lari drein edilebilir.

**DEX Uygulama Notu — Nova Shield icin:**

- Nova Shield settlement sistemi pre/post interactionlari desteklemeli. Bu olmadan kullanicilarin
  DeFi stratejileri entegre etmesi imkansiz.
- Guvenlik: interaction'lar icin whitelist veya simulation zorunlu. Arbitrary call'i acmak
  exploit riskini artiriyor. Her interaction simulation'dan gececek.

---

### [quote.rs] (605 satirlik)

**Amac:** Fiyat teklifi modeli — kullanici order vermeden once fiyat sorabilir.

**Struct/Enum/Trait Listesi:**

- `PriceQuality` (satir 27):
  - `Fast`: En hizli n estimator'un en iyi quote'u.
  - `Optimal` (default): Tum estimator'larin en iyi quote'u.
  - `Verified`: Simulation ile dogrulama (bazilari discard edilebilir).

- `QuoteSigningScheme` (satir 43):
  - `Eip712` (default), `EthSign`: ECDSA schemalar.
  - `Eip1271 { onchain_order: bool, verification_gas_limit: u64 }`: Smart contract imzasi.
    `verification_gas_limit` default: 27_000 (Ambire use case'e gore).
  - `PreSign { onchain_order: bool }`: On-chain pre-sign.

  Kural: ECDSA + onchain_order = HATA ("ECDSA-signed orders cannot be on-chain").
  `additional_gas_amount()`: Eip1271 icin `verification_gas_limit`, digerleri 0.

  `TryFrom<QuoteSigningDeserializationData>` impl: Validation logic tanimlı.

- `OrderQuoteRequest` (satir 122): Quote istegi.
  - `from: Address`, `sell_token, buy_token, receiver`
  - `side: OrderQuoteSide` (flatten)
  - `validity: Validity` (flatten)
  - `app_data: OrderCreationAppData` (flatten, custom deserializer)
  - `sell_token_balance, buy_token_balance, signing_scheme`
  - `price_quality: PriceQuality`
  - `timeout: Option<Duration>` — milisaniye cinsinden

- `OrderQuoteSide` (satir 152):
  - `Sell { sell_amount: SellAmount }` — satarak quote
  - `Buy { buy_amount_after_fee: NonZeroU256 }` — alarak quote
  `kind_and_amount() -> (OrderKind, NonZeroU256)`.

- `Validity` (satir 188):
  - `To(u32)` — belirli Unix timestamp'e kadar
  - `For(u32)` — simden X saniye sonrasina kadar (default: 1800 = 30 dakika)
  `actual_valid_to()`: `For` icin `now_in_epoch_seconds() + valid_for`.
  Custom serde: `validTo` ve `validFor` JSON anahtarlari icin flatten.

- `SellAmount` (satir 307): Untagged enum.
  - `BeforeFee { value: NonZeroU256 }` (JSON key: `sellAmountBeforeFee`)
  - `AfterFee { value: NonZeroU256 }` (JSON key: `sellAmountAfterFee`)

- `OrderQuote` (satir 322): Quote cevabi.
  `sell_token, buy_token, receiver, sell_amount, buy_amount, valid_to, app_data, fee_amount,
  gas_amount, gas_price, sell_token_price, kind, partially_fillable, sell_token_balance,
  buy_token_balance, signing_scheme`.

- `QuoteId = i64` (satir 351): Veritabani ID'si.

- `OrderQuoteResponse` (satir 354): API quote cevabi.
  - `quote: OrderQuote`, `from: Address`, `expiration: DateTime<Utc>`, `id: Option<QuoteId>`
  - `verified: bool`, `protocol_fee_bps: Option<String>` (basis points olarak fee).

- `NativeTokenPrice` (satir 368): `price: f64`.

**Guvenlik Analizi:**

- `NonZeroU256` kullanimi: Sifir miktarli quote engelleniyor. Sifir satis/satin alma safsatasi
  prevention.
- `verification_gas_limit`: EIP-1271 kontrat dogrulama gasinin sinirlanmasi — DoS onlemi.
  27_000 gas bazi buyuk kontratlar icin yetmeyebilir, kullanici override edebilir.
- Timeout mekanizmasi: Eski quote'larla manipulasyon onlenir.

**DEX Uygulama Notu — Nova Shield icin:**

- `BeforeFee`/`AfterFee` ayrimi onemli: "100 USDC harcarken fee dahil mi?" sorusunun yaniti.
  Nova Shield DEX'te bu ayrimi acikca yapmali — kullanici deneyimi icin kritik.
- `protocol_fee_bps`: Fee'nin basis point cinsinden gosterimi — kullanici dostu format.
  Nova Shield API'si de fee'yi bps olarak gostermeli.

---

### [signature.rs] (762 satirlik)

**Amac:** EIP-712, EthSign, EIP-1271 ve PreSign imzalama schemalarini implemente eder.

**Struct/Enum/Trait Listesi:**

- `SigningScheme` (satir 17):
  - `Eip712` (default), `EthSign`, `Eip1271`, `PreSign`.
  `is_ecdsa_scheme()`: Eip712 veya EthSign ise true.
  `try_to_ecdsa_scheme()`: `EcdsaSigningScheme`'e donusturur veya `None`.

- `Signature` (satir 41): Tum imzalama schemalarini kapsayan enum.
  - `Eip712(EcdsaSignature)`: Standart EIP-712 imzasi.
  - `EthSign(EcdsaSignature)`: EIP-191 personal sign.
  - `Eip1271(Vec<u8>)`: Kontrat tarafindan dogrulanacak arbitrary bytes.
  - `PreSign`: On-chain pre-imzalama (bos imza).

  `recover(domain, struct_hash) -> Result<Option<Recovered>>`:
  - Eip712/EthSign: ECDSA recovery -> `Some(Recovered)`
  - Eip1271/PreSign: `None` (kontrat/on-chain dogrulama gerektirir)

  `from_bytes(scheme, bytes) -> Result<Self>`:
  - Eip712/EthSign: Kesinlikle 65 byte zorunlu
  - Eip1271: Keyfi uzunluk
  - PreSign: Bos veya 20 byte (legacy uyumluluk)

  `to_bytes() -> Vec<u8>`:
  - Eip712/EthSign: 65 byte
  - Eip1271: Aynen bytes
  - PreSign: Bos

  `encode_for_settlement(owner) -> Vec<u8>`: On-chain settlement icin kodlama:
  - Eip712/EthSign: 65 byte ECDSA
  - Eip1271: `owner[20] || sig[n]`
  - PreSign: `owner[20]`

  `recover_owner(signature, domain, struct_hash) -> Result<Address>`:
  `encode_for_settlement`'in tersi. Eip1271'de ilk 20 byte = owner.

- `Recovered` (satir 186): `message: B256, signer: Address`.

- `EcdsaSigningScheme` (satir 223): `Eip712` veya `EthSign`.

- `EcdsaSignature` (satir 252): `r: B256, s: B256, v: u8`.
  Default: `r=0, s=0, v=27`.

  `from_bytes(bytes: &[u8; 65]) -> Result<Self>`:
  **v normalizasyonu:**
  - `v=0` -> 27, `v=1` -> 28 (modern EIP-2 format)
  - `v=27` -> 27, `v=28` -> 28 (legacy format)
  - Diger degerler (2, 3, 26, 29, 30, 255...) -> hata
  Bu normalizasyon Solidity `ecrecover`'in v=27/28 beklediginden gerekli.

  `recover(signing_scheme, domain, struct_hash) -> Result<Recovered>`:
  1. `hashed_signing_message` ile mesaj hash'i
  2. `alloy_primitives::Signature::from_raw` ile Alloy imzasina cevir
  3. `recover_address_from_prehash` ile adres recovery

  `sign(scheme, domain, struct_hash, key) -> Self`:
  1. Mesaj hash'i hesapla
  2. `key.sign_hash_sync` ile imzala
  3. `from_bytes` ile v normalizasyonu

  `non_zero() -> Self`: `r=0x01..01, s=0x02..02, v=27` — test amaclı non-zero imza.

- `hashed_eip712_message(domain, struct_hash) -> B256` (satir 270): **Public**.
  ```
  message[0..2]  = [0x19, 0x01]  // EIP-712 magic prefix
  message[2..34] = domain_separator
  message[34..66]= struct_hash
  result = keccak256(message[0..66])
  ```

- `hashed_ethsign_message(domain, struct_hash) -> B256` (satir 280): **Private**.
  ```
  message[0..28] = b"\x19Ethereum Signed Message:\n32"
  message[28..60]= hashed_eip712_message(domain, struct_hash)
  result = keccak256(message[0..60])
  ```
  Onemli tasarim karari: EthSign, raw struct_hash degil EIP-712 hash'ini personal_sign eder.
  Bu kullanicinin ne imzaladigini anlamasi icin daha guvenli.

**Matematiksel Formuller:**

1. **EIP-712 Mesaj Hash:**
   ```
   message = [0x19, 0x01] || domain_separator[32] || struct_hash[32]  (66 bytes toplam)
   eip712_hash = keccak256(message)
   ```

2. **EthSign Mesaj Hash:**
   ```
   inner = eip712_hash (yukaridaki)
   message = "\x19Ethereum Signed Message:\n32"[28] || inner[32]  (60 bytes)
   ethsign_hash = keccak256(message)
   ```

3. **ECDSA v Normalizasyonu:**
   ```
   {0, 27} -> 27  (recovery_id = 0)
   {1, 28} -> 28  (recovery_id = 1)
   diğerleri -> hata
   ```

4. **Settlement Kodlamasi:**
   ```
   EIP712/EthSign: r[32] || s[32] || v[1]             = 65 bytes
   EIP-1271:       owner[20] || arbitrary_sig[n]
   PreSign:        owner[20]
   ```

**Test Coverage:**

- `onchain_signatures_cannot_recover_owners`: PreSign ve Eip1271 None dondurur.
- `signature_from_bytes`: Gecersiz boyutlar hata verir.
- `signature_to_bytes`: Default imza kontrolu.
- `deserialize_and_back`: Round-trip testi, v normalizasyonu dogrulanir.
- `deserialization_errors`: Gecersiz imzalar reddedilir.
- `legacy_presign_signature_format`: 20-byte legacy format kabul edilir.
- `test_ecdsa_signature_recovery`: Gercek private key ile sign/recover testi.
- `ecdsa_signature_v_normalization`: v=0/1 -> 27/28 normalizasyonu.
- `ecdsa_signature_invalid_v_rejected`: 2, 26, 29, 255 vb. reddedilir.

**Guvenlik Analizi:**

- **v Normalizasyonu:** Modern Alloy v=0/1 kullanirken Solidity ecrecover v=27/28 bekler.
  `from_bytes` otomatik normalize eder. Gecersiz v degerler titizce reddedilir.
- **PreSign Guvenlik Modeli:** Kullanici order hash'ini on-chain tx ile onaylar. Bu tx
  private key'le imzalanmistir — "imza" minimalist ama guvenlidir. On-chain kayit silinemez.
- **EIP-1271 Guvenlik:** Kontrat herhangi bir logic implemente edebilir. Kontrat hacklenirse
  imzalar gecersiz olabilir. Simulation bu riski azaltir.
- **Replay Protection:** DomainSeparator, chain_id + contract_address icerir.
- **ECDSA Malleability:** Alloy lower-s normalizasyonu dahili olarak handle eder.

**DEX Uygulama Notu — Nova Shield icin:**

- **EIP-712 zorunlu:** Tum order imzalamasinda EIP-712 kullanmali. Raw hash imzalamak
  kullanicinin ne imzaladigini anlamamasina yol acar.
- **PreSign mekanizmasi:** Multisig kullanicilari (Safe, Gnosis) icin kritik.
  Bu olmadan multisig kullanicilari DEX kullanamiyor.
- **EIP-1271:** Hesap soyutlama (AA/ERC-4337) icin zorunlu. Nova Shield ilk gunden desteklemeli.
- **v Normalizasyonu:** Kendi imzalama katmaninizda ayni sorunu yasarsınız. Her zaman v'yi
  27/28 formatina normalize edin.
- **Nasil daha iyi yapabiliriz:** EIP-1271 icin `verification_gas_limit`'i simulation ile otomatik
  belirleme. CoW kullaniciya birakiyor — UX sorununa yol acıyor.

---

### [solver_competition.rs] (~225 satirlik)

**Amac:** Solver yarismasi sonucunu DB'de ve API'de modeller.

**Struct/Enum/Trait Listesi:**

- `SolverCompetitionDB` (satir 14): DB'de saklanan yarisma verisi.
  - `auction_start_block: u64`
  - `competition_simulation_block: u64`
  - `auction: CompetitionAuction`
  - `solutions: Vec<SolverSettlement>`

- `SolverCompetitionAPI` (satir 24): API endpoint'ine donen genis format.
  - `auction_id: AuctionId`
  - `transaction_hashes: Vec<B256>`
  - `common: SolverCompetitionDB` (flatten)

- `CompetitionAuction` (satir 33):
  - `orders: Vec<OrderUid>` — auction'a dahil orderlar
  - `prices: BTreeMap<Address, U256>` — native token fiyatlari (BTreeMap = sorted = deterministic)

- `SolverSettlement` (satir 44): Bir solver'in sundugu cozum.
  - `solver: String` — solver ismi
  - `solver_address: Address`
  - `score: Option<Score>` (flatten)
  - `ranking: usize` (1 = kazanan)
  - `clearing_prices: BTreeMap<Address, U256>`
  - `orders: Vec<Order>`
  - `is_winner: bool`, `filtered_out: bool`

- `Score` (satir 62): Solver cozumunun puani.
  - `Solver(U256)` — solver belirledi
  - `Protocol(U256)` — protocol hesapladi (objective function'a esit)
  - `ProtocolWithSolverRisk(U256)` — success_probability dahil
  - `Discounted(U256)` — DEPRECATED, backward compat
  Default: `Protocol(U256::ZERO)`
  `score() -> U256`: Ic deger.

- `Order` (satir 102): Settlement'taki order (untagged enum).
  - `Colocated { id, sell_amount, buy_amount }` — gercek miktarlar
  - `Legacy { id, executed_amount }` — eski format

**Guvenlik Analizi:**

- `filtered_out`: Simulation'da reddedilen cozumler isaretlenir. Kotu niyetli solver'larin
  yanlis cozum sunup reward almasi engellenir.
- `clearing_prices`: Settlement reference fiyatlari. Yanlis clearing prices yanlis fee hesabina yol
  acar. Circuit breaker on-chain sonuclarla karsilastirir.
- `Discounted` deprecated: Backward compat icin tutuluyor; yeni entegrasyonlar kullanmamali.

**DEX Uygulama Notu — Nova Shield icin:**

- Nova Shield batch auction sisteminde bu yapi aynen kullanilabilir.
- `competition_simulation_block` kritik: Simulation bloku farkliligi clearing price'lari etkiler.
  Settlement bloku ile simulation bloku arasindaki fark minimize edilmeli.
- `clearing_prices` BTreeMap: Deterministic JSON ciktisi saglar — onemli.

---

### [solver_competition_v2.rs] (161 satirlik)

**Amac:** Solver yarisma API'sinin v2 modeli — daha fazla detay, reference_scores eklenmis.

**Struct/Enum/Trait Listesi:**

- `Response` (satir 13): V2 API cevabi.
  - `auction_id: AuctionId`
  - `auction_start_block: i64` (v1'de u64, burada i64 — PostgreSQL uyumlulugu)
  - `auction_deadline_block: i64` (yeni)
  - `transaction_hashes: Vec<B256>`
  - `reference_scores: BTreeMap<Address, U256>` (yeni — her solver'in referans skoru)
  - `auction: Auction`, `solutions: Vec<Solution>`

- `Auction` (satir 27): V2 auction verisi (v1 ile ayni yapida).

- `Solution` (satir 36): V2 cozum verisi.
  - `solver_address, score: U256, ranking: i64`
  - `clearing_prices, orders, is_winner, filtered_out`
  - `tx_hash: Option<B256>` (yeni)
  - `reference_score: Option<U256>` (yeni)

- `Order` (satir 52): V2 order (v1 Colocated gibi).
  - `id, sell_amount, buy_amount, buy_token, sell_token` (token adresleri v2'de eklendi)

  V1'deki `Legacy` format kaldırildi.

**Guvenlik Analizi:**

- `reference_score`: Kazanamayan solver'larin kalitesi izleniyor. Adil rekabet degerlendirmesi.
- v2'de `i64` blok numaralari: PostgreSQL BIGINT uyumlulugu. Negatif blok anlamsiz ama DB
  kısitlamasından gelen tasarim karari.

**DEX Uygulama Notu — Nova Shield icin:**

- `reference_score` kavrami: Her solver'in ne kadar geri kaldigi olculebilir. Solver kalitesini
  zaman icinde track etmek icin degerli.
- `tx_hash` per-solution: Hangi cozumun hangi tx ile settle edildigini takip eder.

---

### [trade.rs] (148 satirlik)

**Amac:** Gerceklesen trade'in on-chain verisini modeller.

**Struct/Enum/Trait Listesi:**

- `Trade` (satir 16): Order settlement sonucu.
  - `block_number: u64, log_index: u64` — benzersiz cift index
  - `order_uid: OrderUid`
  - `buy_amount: BigUint, sell_amount: BigUint, sell_amount_before_fees: BigUint`
    (`DisplayFromStr` serde — string olarak serialize)
  - `owner, buy_token, sell_token: Address`
  - `tx_hash: Option<B256>`
  - `executed_protocol_fees: Vec<ExecutedProtocolFee>`

  `BigUint` kullanimi: Keyfi buyuklukte integer. U256 yeterli olurdu ama BigUint hata toleransi sagliyor.

**Test Coverage:**

- `deserialization_and_back`: Surplus, Volume, PriceImprovement — uc fee politikasinin tam round-trip testi.
- `debug_trade_data`: Default Trade dbg ciktisi.

**Guvenlik Analizi:**

- `(block_number, log_index)` cift index: Trade'i benzersiz tanimlayan ikili. Tekrar isleme onlenir.
- `sell_amount_before_fees` vs `sell_amount`: Fee dogrulama icin ikisi gerekli.
  `sell_amount = sell_amount_before_fees - total_fees` dogrulanabilir.

**DEX Uygulama Notu — Nova Shield icin:**

- `(block_number, log_index)` birincil anahtar olarak kullanmak uygun.
- `executed_protocol_fees` listesi: Hiyerarsik fee sistemi kurulmak istenirse bu yapi uygun.

---

### [time.rs] (22 satirlik)

**Amac:** Zaman yardimci fonksiyonlari.

**Fonksiyonlar:**

- `now_in_epoch_seconds() -> u32` (satir 8):
  `SystemTime::now()` -> Unix epoch saniye -> u32 cast.
  `expect` ile panic: Epoch oncesi veya u32 overflow. 2106'ya kadar guvenli.

- `timestamp_after_duration(timestamp: u32, duration: Duration) -> u32` (satir 19):
  `timestamp.saturating_add(duration.as_secs().try_into().unwrap_or(u32::MAX))`
  Saturating semantik — overflow durumunda `u32::MAX`, panic yok.

**Guvenlik Analizi:**

- `now_in_epoch_seconds` `expect` ile panic: Sunucu saati yanlis ayarliysa tehlike. Production'da
  imkansiz ama `Result` donmek daha dogru olurdu.
- u32::MAX = 4294967295 = yaklasik Subat 2106 overflow noktasi.

**DEX Uygulama Notu — Nova Shield icin:**

- Nova Shield L1 icin `u64` timestamp dusunulmeli. EVM u32 kullaniyor ama L1 kendi zincirinde
  u64 veya daha buyuk kullanabilir.

---

### [debug_report.rs] (149 satirlik)

**Amac:** Order yasam dongusu boyunca debug verisi toplar.

**Struct/Enum/Trait Listesi:**

- `DebugReport` (satir 14): Bir order'in tam debug raporu.
  - `order_uid: OrderUid`, `order: Order`
  - `events: Vec<Event>`, `auctions: Vec<Auction>`, `trades: Vec<Trade>`

- `Event` (satir 23): Order yasam dongusu olayi.
  - `label: String, timestamp: DateTime<Utc>, reason: Option<String>`

- `Auction` (satir 33): Bir auction'daki order gorunumu.
  - `id: i64, block: i64, deadline: i64`
  - `native_prices: HashMap<Address, BigDecimal>`
  - `proposed_solutions: Vec<ProposedSolution>`
  - `executions: Vec<Execution>`
  - `settlement_attempts: Vec<SettlementAttempt>`
  - `fee_policies: Vec<FeePolicy>`

- `ProposedSolution` (satir 49): Solver'in onerdigi cozum (debug).
  `solution_uid, ranking, solver, is_winner, filtered_out, score, executed_sell, executed_buy`

- `Execution` (satir 66): Gerceklesen settlement.
  `executed_fee, executed_fee_token, block_number, protocol_fees: Vec<ProtocolFee>`

- `ProtocolFee` (satir 78): `token: Address, amount: BigDecimal`.

- `Trade` (debug_report icindeki, satir 89): Debug trade versiyonu.
  `block_number, log_index, buy_amount, sell_amount, sell_amount_before_fees, tx_hash, auction_id`

- `SettlementAttempt` (satir 108): Settlement denemesi.
  `solver, solution_uid, start_timestamp, end_timestamp, start_block, end_block, deadline_block, outcome`

- `FeePolicy` (satir 124): Fee politikasi debug versiyonu.
  `kind: FeePolicyKind`, opsiyonel faktorlar.

- `FeePolicyKind` (satir 141): `Surplus`, `Volume`, `PriceImprovement`.

**Guvenlik Analizi:**

- Debug report hassas bilgi icerir (solver stratejisi, fiyat gecmisi). Sadece yetkili
  endpoint'lerden erisime izin verilmeli.
- `skip_serializing_if = "Option::is_none"`: Gereksiz null alanlar yazilmaz. Bant genisligi tasarrufu
  ve JSON temizligi.

**DEX Uygulama Notu — Nova Shield icin:**

- Bu yapi Nova Shield izleme/debug altyapisi icin ideal referans. Her order'in tam yasam dongusu
  takip edilmeli: olusturma -> auction'a ekleme -> cozum onerisi -> settlement veya iptal.
- `SettlementAttempt` yapisi: Basarisiz settlement denemeleri debug icin kritik.

---

## Genel Mimari Degerlendirme

### CoW Protocol Model Katmaninin Gucleri

1. **EIP-712 Type Safety:** `hash_struct()` Solidity sozlesmesiyle byte-by-byte uyumlu.
   `TYPE_HASH` gomelidir. Off-chain/on-chain tutarlilik garanti.

2. **OrderUid Bilesik Yapi:** 32-byte hash + 20-byte owner + 4-byte valid_to = 56 byte.
   Hem benzersiz hem de icerisinde bilgi tasir. URL-safe hex encoding.

3. **Fee Politikasi Zenginligi:** Uc farkli fee modeli. Solver optimizasyonu farkli sekillerde
   tesvik ediliyor.

4. **Signature Cok-Katmanlilik:** EIP-712, EthSign, EIP-1271, PreSign — tum Ethereum kullanici
   tiplerine uygun.

5. **Builder Pattern:** `OrderBuilder` test yazmayı kolaylastiriyor. Fluent API.

6. **BTreeMap Kullanimi:** `clearing_prices` ve `prices` BTreeMap — deterministic JSON. Audit
   ve dogrulama acisindan kritik.

### Potansiyel Zayifliklar

1. **f64 fee factors:** `FeePolicy::Surplus { factor: f64 }` — floating point. Off-chain icin
   kabul edilebilir ama on-chain hesaplamalarla uyumsuzluk riski.

2. **u32 timestamp:** 2106 limiti. Kisa vadeli sorun degil ama yeni L1 icin dusunulmeli.

3. **Magic BUY_ETH_ADDRESS:** `0xEEEE...EE` sezgisel degil. Explicit enum daha type-safe olurdu.

4. **Tip duplikasyonu:** `solver_competition.rs` vs `solver_competition_v2.rs` — iki versiyon
   ayri dosyalarda. Versioning stratejisi daha temiz olabilirdi.

5. **AuctionId = i64:** Negatif ID anlamsiz. PostgreSQL uyumlulugu nedeniyle secilmis.

---

## Nova Shield DEX icin Kritik Eylem Plani

### Dogrudan Kopyalanabilecekler
- `OrderUid` yapi: 56-byte bilesik kimlik (hash + owner + valid_to)
- `DomainSeparator::new(chain_id, contract_address)` implementasyonu
- `hash_struct()` 416-byte buffer yaklasimi (Solidity ile byte-by-byte uyum saglanmali)
- `hashed_eip712_message()` formulu: `0x19 0x01 || domain || struct_hash`
- Signature v normalizasyon mantigi: `{0,27}->27, {1,28}->28, digerleri->hata`
- `TokenPair` siralama ve null guvenlik
- `FeePolicy` uc-katli fee modeli (surplus/volume/price_improvement)
- Pre/post interaction modeli (`InteractionData`)
- `OrderKind` semantigi: Buy=minimum_garantisi, Sell=maksimum_siniri
- `OrderClass` uclemesi: Market/Limit/Liquidity
- `SellTokenSource`/`BuyTokenDestination` Balancer-stili balance kaynagi

### Nova Shield'de Farkli Yapilmasi Gerekenler
- `f64` fee factor yerine fixed-point (U256 ile basis points 1/10000)
- `u32` timestamp yerine `u64` (Layer 1 uzun omurlu olacak, EVM uyumlulugu gerekiyorsa u32)
- `BUY_ETH_ADDRESS` magic yerine explicit `TokenSpec::Native` enum varyanti
- EIP-1271 `verification_gas_limit` icin simulation bazli otomatik belirleme
- `AuctionId` i64 yerine `u64` (negatif anlamsiz, DB katmaninda i64 map edilebilir)

### Nova Shield Icin Eklenecek Ozellikler
- Cross-chain order routing: multi-chain batch auction destegi
- ZK-proof imzalama: Nova Shield MPC/threshold yapisiyla entegre imzalar
- Privacy katmani: on-chain settlement'ta ZK batch swap (Penumbra'dan esinlenerek)
- MEV korumasinda zaman-kilidi: batch auction yeterli ama ZK gizliligi eklenebilir
- Daha zengin OrderClass: Cross-chain, Flash-loan-backed, Conditional orderlar
