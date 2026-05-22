# BidMart Wallet Service Optimization

Dokumen ini menjelaskan optimasi performa yang dilakukan pada hot path wallet service, khususnya simulasi bidding wallet:

```text
25000 wallets x 80 bid settlement cycles = 4000000 wallet mutations
```

Hot path yang diprofiling adalah operasi domain wallet untuk bidding:

- `bid`: memindahkan saldo aktif ke saldo tertahan.
- `release`: mengembalikan saldo tertahan ke saldo aktif untuk bid yang kalah.
- `convert`: menyelesaikan saldo tertahan untuk bid yang menang.

## Profiling Baseline

Sebelum optimasi, hasil profiling menunjukkan:

```text
Total time:          1.123932786s
Wallet mutations:     4000000
Per wallet mutation:  280 ns
```

Flamegraph menunjukkan waktu banyak habis di pembuatan audit transaction, terutama:

- `WalletTransaction::new`
- `uuid::Uuid::new_v4().to_string()`
- `uuid::fmt::format_hyphenated`
- alokasi `String` untuk `user_id` dan `role` pada setiap mutasi

Artinya, bottleneck utama bukan arithmetic saldo, tetapi overhead metadata transaksi.

## Optimasi Yang Dilakukan

### 1. Lazy Transaction ID

Sebelumnya, setiap mutasi wallet langsung membuat UUID string:

```rust
id: uuid::Uuid::new_v4().to_string()
```

Sekarang, `WalletTransaction.id` disimpan sebagai `uuid::Uuid`, dan transaksi domain baru memakai `Uuid::nil()` terlebih dahulu. UUID final baru dibuat saat transaksi benar-benar dipersist ke database.

Dampaknya:

- Hot path domain tidak lagi membuat UUID untuk setiap mutasi.
- Hot path domain tidak lagi melakukan formatting UUID ke string.
- Database tetap menyimpan ID transaksi sebagai UUID string.
- HTTP response tetap mengembalikan ID transaksi sebagai string.

### 2. `Arc<str>` Untuk Metadata Transaksi

Sebelumnya, setiap `WalletTransaction` melakukan alokasi baru:

```rust
user_id: user_id.to_string()
role: role.to_string()
```

Sekarang, `Wallet` dan `WalletTransaction` memakai `Arc<str>` untuk `user_id` dan `role`.

Dampaknya:

- Mutasi wallet cukup clone pointer, bukan allocate string baru.
- Metadata tetap bisa dibaca sebagai string di boundary HTTP dan persistence.
- Kontrak eksternal tidak berubah.

## Hasil Setelah Optimasi

Setelah optimasi pertama, hasil turun dari `280 ns` ke `261 ns` per mutasi. Ini belum cukup karena UUID generation dan alokasi string masih terjadi.

Setelah optimasi lazy ID dan `Arc<str>`, hasil profiling menjadi:

```text
Total time:          114.986266ms
Wallet mutations:     4000000
Per wallet mutation:  28 ns
```

Perbandingan:

| Metrik | Sebelum | Sesudah | Improvement |
|---|---:|---:|---:|
| Total time | 1.123932786s | 114.986266ms | ~89.8% lebih cepat |
| Per mutation | 280 ns | 28 ns | ~90.0% lebih cepat |

Target minimal improvement adalah 50%. Hasil akhir melewati target tersebut secara signifikan.

## Dampak Ke Modul Lain

Perubahan ini memengaruhi tipe internal, tetapi tidak mengubah kontrak eksternal.

Yang berubah secara internal:

- `WalletTransaction.id` berubah dari `String` menjadi `Uuid`.
- Transaksi domain yang belum dipersist memakai `Uuid::nil()`.
- `Wallet.user_id`, `Wallet.role`, `WalletTransaction.user_id`, dan `WalletTransaction.role` memakai `Arc<str>`.

Boundary yang tetap kompatibel:

- HTTP DTO tetap mengembalikan `id`, `userId`, dan `role` sebagai string.
- Repository tetap menyimpan ID transaksi sebagai string di database.
- Service layer tetap menerima parameter string seperti sebelumnya.
- Riwayat transaksi yang dibaca dari database tetap memiliki UUID final.

Catatan penting:

Jika ada kode baru yang langsung memakai `tx.id` dari hasil `wallet.bid()`, `wallet.release()`, atau `wallet.convert()` sebelum transaksi dipersist, ID tersebut masih `Uuid::nil()`. ID final tersedia setelah transaksi masuk repository atau dibaca dari history.
