# Stockbit rate-limiter — findings & fetch strategy

Repo: `<repo>` · Probe: 2026-10-07 17:56–21:01 WIB · Read-only, HTTP-only (tidak menyentuh MongoDB)
Artifacts: `/tmp/rl_results/{summary,summaryC,summaryD}.json`, `probe.jsonl`, `phase{B,C,D}.log`

## 1. Ringkasan eksekutif (5 baris)

1. Limiter Stockbit = **kuota per-akun (token) per-endpoint**, bukan per-IP: jalur proxy (cf-ray CGK) dan direct (cf-ray SIN) memberi hasil stub identik.
2. Tidak ada error: selalu **HTTP 200 + body stub kosong** (`from/to=""`). Header `x-rate-limit-*` **statis** (limit 10 / remaining 8 / reset 0) dan **tidak bisa dipakai prediksi**.
3. Tidak ada celah: ganti token, ganti IP, reuse/keealive session, dan semua variasi parameter (limit/harga board/investor/range) **tetap stub**. Satu-satunya "pintu" yang tidak ter-throttle adalah **endpoint lain** (brokerdistribution, tradebook, orderbook).
4. Bucket token **sangat kecil**: dari kondisi idle penuh (dijeda 180s) hanya **~3 request** lolos lalu stub; pemulihan setelah throttle ≈ **16–48 s**.
5. Penyebab backfill cuma 8–15%: **beberapa konsumen berbagi satu token** (fetch harian + loop backfill + cron 15-menit + probe manual) saling men-starve. Laju berapapun di atas ~0.17 rps nyaris selalu stub.

## 2. Tabel hasil eksperimen

Semua jam WIB. "stub" = HTTP 200 body kosong (throttled). Endpoint default `GET https://exodus.stockbit.com/marketdetectors/{sym}` params `transaction_type=TRANSACTION_TYPE_NET&market_board=MARKET_BOARD_REGULER&investor_type=INVESTOR_TYPE_ALL&limit=100&from=<d>&to=<d>`.

| # | Waktu | Perlakuan (perintah inti) | Sukses/stub | Catatan |
|---|-------|---------------------------|-------------|---------|
| R1 | 17:56 | 10 req berurutan @1/s, proxy | **10 ok / 0 stub** | Header tiap kali `limit=10, remaining=8, reset=0` (statis) |
| R2 | 17:57 | 5 req @~2/s: proxy lalu **direct** | 5 ok / 0 stub | Proxy CGK **dan** direct SIN sama-sama 200 + data asli |
| B0 | 20:24:20 | baseline: BBRI 2026-10-06 (hari bursa) & 2026-10-04 (Minggu) | keduanya **ok** | Hari Minggu meng-echo range → detector stub terbukti benar (bukan false-positive) |
| B1 | 20:24:21 | **burst @8 rps** | sent 9, **first_stub di req #2** | Token sudah terpakai pasca-fetch-harian |
| B2 | 20:24:29 | rotate IP: proxy→direct→proxy→direct | **stub ke-4** | CGK & SIN identik → **bukan per-IP** |
| B3 | 20:24:31 | md throttled → tes endpoint lain | **bd ok, tb ok, orderbook HTTP 200** | Throttle **per-endpoint** |
| B4 | 20:24:33 | variasi param: limit 1/9999, board tunai/nego, investor asing, range multi-hari | **semua stub** | Tidak ada parameter escape |
| B5 | 20:24:36 | Session keep-alive vs fresh | **semua stub (8/8)** | Session tidak berpengaruh |
| B6 | 20:24:44 | **`token_refresh.py`** lalu retry | refresh rc=0, **before stub=T, after stub=T** | Ganti token **tidak reset** budget |
| B7 | 20:24:47 | recovery poll | **16.5 s** | Cepat pulih bila berhenti menembak |
| B8 | 20:25:03 | burst @2 rps | sent 7, **first_stub req #3** | Laju lebih rendah tidak menolong |
| C1 | 20:30 | 45 s @8 rps | 6/95 ok (**6.3%**) | first_stub @1.5 s |
| C2 | 20:31 | 45 s @4 rps | 3/87 (**3.4%**) | first_stub @0.6 s |
| C3 | 20:32 | 45 s @2 rps | 2/90 (**2.2%**) | first_stub @0.4 s |
| C4 | 20:33 | 45 s @1 rps | 1/45 (**2.2%**) | first_stub @1.3 s |
| C5 | 20:35 | 45 s @0.5 rps | 2/23 (**8.7%**) | first_stub @0.3 s |
| C6 | 20:36 | 4 worker @4 rps total, 45 s | 3/179 (**1.7%**) | **Konkurensi memperburuk** |
| C7 | 20:37–20:39 | refill pasca-throttle keras (3 trial) | **30.7 / 48.4 / 43.9 s** | Konsisten puluhan detik |
| C8 | 20:39 | md throttled → endpoint lain (3x tiap) | **bd 3/3 ok, tb 3/3 ok, orderbook 200×3** | Konfirmasi per-endpoint |
| D1 | 20:53:54 | pesaing dijeda 180s, **burst @4 rps** | pola **`oooS`** = 3 ok lalu stub di #4 | Bucket idle ≈ 3 request |
| D2 | 20:53:59 | lanjut tanpa jeda | ~3 ok lagi dalam ~6 s, lalu stub beruntun | Bucket refill kecil & cepat habis |
| D3 | 20:56–21:01 | **konsumen tunggal** `sbfill6` @pace 6 (0.167 rps), tanpa probe lain | **18 ok / 16 stub (53%)** | Bahkan 1 req/6 s ≈ setengah ke-throttle |

> Konfounder penting: pada 20:24–20:39 berjalan bersamaan **fetch harian** (`fetch_sb_market_detectors.py --pace 2.5`, mulai 19:00, selesai ~20:30) dan loop **`sbfill6`** (`PACE=6 ROUNDS=6 REST=600`, tmux). `sbfill6` sendiri mencatat ~29% ok saat probe-ku juga menembak; naik ke ~53% setelah probe berhenti. Jadi angka "terkontensi" (B/C) = lantai bawah, angka D3 = gambaran konsumen tunggal.

### Perintah yang dipakai (persis)
```bash
# recon & harness
cd <repo>
./.venv/bin/python /tmp/rl_recon.py
RL_OUT=/tmp/rl_results/probe.jsonl ./.venv/bin/python /tmp/rl_probe.py headers --date 2026-10-06 --gap 1.0
# baterai penuh (jalan otomatis setelah >=20:02 & fetch harian selesai)
setsid nohup ./.venv/bin/python -u /tmp/rl_phaseB.py > /tmp/rl_results/phaseB_stdout.log 2>&1 &
setsid nohup ./.venv/bin/python -u /tmp/rl_phaseC.py > /tmp/rl_results/phaseC_stdout.log 2>&1 &
setsid nohup ./.venv/bin/python -u /tmp/rl_phaseD.py > /tmp/rl_results/phaseD_stdout.log 2>&1 &
# cek jawaban mentah
tail -f /tmp/rl_results/phase{B,C,D}.log
# contoh 1 request
curl -sS -x socks5h://<proxy-host>:1080 \
  -H "authorization: Bearer $BEARER_TOKEN" -H 'user-agent: curl/8.0.0' \
  'https://exodus.stockbit.com/marketdetectors/BBRI?transaction_type=TRANSACTION_TYPE_NET&market_board=MARKET_BOARD_REGULER&investor_type=INVESTOR_TYPE_ALL&limit=100&from=2026-10-06&to=2026-10-06' \
  -D - -o /tmp/body.json
```

## 3. Mekanisme limiter (kesimpulan + bukti)

**Model:** token-bucket/kuota kecil **per akun (access token) per endpoint**, dihitung di sisi Stockbit, dengan refill lambat dan "hukuman" setelah burst.

Bukti:
- **Per-akun, bukan per-IP** — proxy (CGK) dan direct (SIN) memberi stub yang sama persis (B2); IP yang berbeda tidak memisahkan budget.
- **Per-endpoint** — saat `marketdetectors` di-throttle keras, `order-trade/broker/distribution`, `order-trade/trade-book`, dan `company-price-feed/.../orderbook` tetap 200 + data asli (B3, C8). `brokerdistribution` bahkan tidak mengirim header `x-rate-limit-*` sama sekali.
- **Bucket kecil** — setelah 180 s benar-benar idle, hanya ~3 request lolos sebelum stub (D1). Refill cepat habis lagi (D2).
- **Header menyesatkan** — `limit=10, remaining=8, reset=0` tidak pernah berubah, bahkan tepat saat stub (semua log). Tidak ada 429; throttle = 200 + stub.
- **Tidak bisa di-reset** — refresh token (B6), ganti IP (B2), session baru (B5), variasi parameter (B4) semuanya gagal.
- **Deteksi stub valid** — hari Minggu (upstream kosong) meng-echo `from/to` → `is_stub=False`; artinya guard `lib.is_stub_payload` benar memisahkan "memang kosong" dari "throttled" (B0).

## 4. Rekomendasi strategi fetch (konkret)

Angka acuan dari data: laju raw >0.25 rps ⇒ first stub <1.5 s; konsumen tunggal 0.167 rps ⇒ ~53% lolos; recovery pasca-throttle 16–48 s; konkurensi 4 worker ⇒ 1.7%.

1. **Satu limiter global untuk semua konsumen Stockbit.** Bungkus semua pemanggil `marketdetectors` (fetch harian, loop backfill, cron 15-menit, run manual) dengan satu lock + token-bucket bersama (mis. `/tmp/sb_marketdetectors.lock` + `flock`). Jangan pernah ada dua fetcher `marketdetectors` jalan bersamaan. Ini perbaikan tunggal paling berdampak (C6 vs D3).
2. **Pace konservatif, 1 worker.** Target **≤0.15 rps (jeda ≥6–7 s antar request)** untuk `marketdetectors`; jangan lewat ~0.25 rps. Ekspektasi: ~50–70% lolos → **≈0.08–0.12 baris sukses/detik ≈ 280–430 stock-hari/jam**.
3. **Jangan retry di dalam satu pass.** Saat stub, jangan ulang (kode sekarang: `STUB_BACKOFF=[5]` lalu backfill mengulang pair di pass yang sama). Batalkan pass setelah N stub beruntun (sudah ada `--throttle-limit 12`), istirahat ≥600–3600 s, lalu lanjut (resumable).
4. **Pisahkan lane per-endpoint.** `brokerdistribution`, `tradebook`, `orderbook` tidak kena throttle marketdetectors → jalankan di lane terpisah/paralel tanpa ikut menunggu budget `marketdetectors`. (Pertahankan pemisahan grup sb/idx/yf di `run_all_parallel.sh`; jangan gabung antrean.)
5. **Backfill di jendela idle besar** (mis. 06:00–18:00, dan hindari 18:00–20:00 — sudah benar), **satu konsumen saja**, sambil **mematikan cron backfill 15-menit yang tumpang-tindih** selama backfill besar. Hentikan `--pace` kecil berulang; lebih baik satu run panjang berpaced.
6. **Buang rencana rotasi IP / rotasi token / parameter bypass** dari roadmap: terbukti tidak berfungsi (B2, B4, B6).
7. **Perkiraan throughput realistis:** satu hari bursa penuh (958 saham) ≈ **2,5–4 jam** akses eksklusif berpaced (~0.08–0.12 sukses/s). Karena hit-rate ~50%, anggarkan 2–3 pass dengan jeda 10–60 menit untuk menutup gap. Ini jauh lebih baik daripada kondisi sekarang (2–8% karena rebutan).

## 5. Yang TIDAK bisa ditembus (dan kenapa)

- **Kuota per-akun per-endpoint**: dihitung server-side; tidak ada flag/parameter yang melewatinya (`limit`, `market_board`, `investor_type`, `from/to`, "uncapped" — semua stub; B4).
- **Ganti IP**: bukan pembatas IP — proxy CGK dan direct SIN identik; proxy tambahan tidak menambah budget (B2).
- **Ganti token / refresh**: budget melekat pada akun, refresh tidak me-reset (B6).
- **Session/keep-alive**: tidak berpengaruh (B5).
- **Header `x-rate-limit-*`**: nilai statis (10/8/0), tidak melacak pemakaian → tidak bisa jadi sinyal pacing (semua run).
- **Error 429**: tidak pernah muncul; throttle selalu menyamar sebagai 200 + body kosong → wajib deteksi lewat echo `from/to` (sudah ada di `lib.is_stub_payload`).
- **Burst > ~0.25 rps**: langsung stub; tidak ada jalan "borrow" budget dari endpoint lain untuk `marketdetectors` (independen, jadi tidak saling bantu).

**Kesimpulan satu kalimat:** tidak ada cara "menembus" limiter; kemenangan datang dari **membuat satu konsumen berpaced + menyerialkan semua pemakai token**, bukan dari rotasi/bypass.
