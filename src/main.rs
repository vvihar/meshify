use clap::{Parser, ValueEnum};
use proj::Proj;
use std::error::Error;
use std::fs::File;
use std::path::PathBuf;

/// 入力座標の測地系
#[derive(Copy, Clone, Debug, ValueEnum)]
enum Datum {
    WGS,
    JGS,
}

/// 計算するメッシュのレベル
#[derive(Copy, Clone, Debug, ValueEnum)]
enum MeshLevel {
    Standard,
    Half,
    Quarter,
    Eighth,
}

/// CSVファイル内の緯度経度に地域メッシュコードを付与するツール
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// 緯度が含まれる列名
    #[arg(long)]
    lat: String,

    /// 経度が含まれる列名
    #[arg(long)]
    lon: String,

    /// 入力座標の測地系
    #[arg(short, long, default_value = "wgs")]
    datum: Datum,

    /// 出力先のファイルパス (指定しない場合は、<入力ファイル名>_mesh.csv に出力)
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// 計算するメッシュのレベル
    #[arg(short, long, default_value = "standard")]
    level: MeshLevel,

    /// 入力CSVファイルのパス
    #[arg()]
    input_file: PathBuf,
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();
    let mut reader = csv::Reader::from_path(&args.input_file)?;

    // 出力ファイルパスの決定
    let output_path = args.output.unwrap_or_else(|| {
        let mut input_path = args.input_file.clone();
        let file_stem = input_path.file_stem().unwrap().to_string_lossy();
        input_path.set_file_name(format!("{}_mesh.csv", file_stem));
        input_path
    });

    let mut writer = csv::Writer::from_writer(File::create(output_path)?);

    let headers = reader.headers()?.clone();
    let lat_idx = headers
        .iter()
        .position(|h| h == args.lat)
        .ok_or("緯度列が見つかりません")?;
    let lon_idx = headers
        .iter()
        .position(|h| h == args.lon)
        .ok_or("経度列が見つかりません")?;

    let mut new_headers = headers.iter().map(String::from).collect::<Vec<String>>();
    new_headers.push("mesh_code".to_string());
    writer.write_record(&new_headers)?;

    // 日本測地系 (Tokyo Datum, EPSG:4301) → 世界測地系 (WGS84, EPSG:4326)
    let proj = Proj::new_known_crs("EPSG:4301", "EPSG:4326", None)?;

    for result in reader.records() {
        let mut record = result?;

        // readerから現在の行番号を取得する
        let line_number = record.position().map(|p| p.line()).unwrap_or(0);

        let lat_str = &record[lat_idx];
        let lat: f64 = match lat_str.trim().parse() {
            Ok(val) => val,
            Err(_) => {
                // パース失敗時に警告を出し、この行の処理をスキップする
                eprintln!("[警告] {}行目: 緯度の値「{}」が不正なため、この行をスキップします。", line_number, lat_str);
                continue;
            }
        };

        let lon_str = &record[lon_idx];
        let lon: f64 = match lon_str.trim().parse() {
            Ok(val) => val,
            Err(_) => {
                eprintln!("[警告] {}行目: 経度の値「{}」が不正なため、この行をスキップします。", line_number, lon_str);
                continue;
            }
        };

        let (wgs_lat, wgs_lon) = match args.datum {
            Datum::JGS => {
                // PROJは (経度, 緯度) の順
                let (converted_lon, converted_lat) = proj.convert((lon, lat))?;
                (converted_lat, converted_lon)
            }
            Datum::WGS => (lat, lon),
        };

        let mesh_code = get_mesh_code(wgs_lat, wgs_lon, args.level);

        record.push_field(&mesh_code);
        writer.write_record(&record)?;
    }

    writer.flush()?;
    Ok(())
}

/// 世界測地系の緯度経度から地域メッシュコードを計算
fn get_mesh_code(lat: f64, lon: f64, level: MeshLevel) -> String {
    // --- 基準地域メッシュ（3次メッシュ）の計算 ---
    let lat_min = lat * 60.0;
    let (p, a_rem) = ((lat_min / 40.0).floor(), lat_min % 40.0);

    let (q, b_rem) = ((a_rem / 5.0).floor(), a_rem % 5.0);

    let lat_sec_in_b = b_rem * 60.0;
    let (r, c_rem) = ((lat_sec_in_b / 30.0).floor(), lat_sec_in_b % 30.0);

    let lon_deg_rem = lon - lon.floor();
    let u = lon.floor() - 100.0;

    let lon_min_rem = lon_deg_rem * 60.0;
    let (v, g_rem) = ((lon_min_rem / 7.5).floor(), lon_min_rem % 7.5);

    let lon_sec_in_g = g_rem * 60.0;
    let (w, h_rem) = ((lon_sec_in_g / 45.0).floor(), lon_sec_in_g % 45.0);

    // まず、変更しないベースとなるコードを mutable な String として作成
    let mut code = format!(
        "{}{}{}{}{}{}",
        p as u32, u as u32, q as u32, v as u32, r as u32, w as u32
    );

    // 目的のレベルに達していない場合は、計算を続行する
    if let MeshLevel::Standard = level {
        return code;
    }

    // --- 2分の1地域メッシュの計算 ---
    let (s, d_rem) = ((c_rem / 15.0).floor(), c_rem % 15.0);
    let (x, i_rem) = ((h_rem / 22.5).floor(), h_rem % 22.5);
    let m = (s * 2.0) + x + 1.0;
    code.push_str(&(m as u32).to_string()); // 計算結果を追記

    if let MeshLevel::Half = level {
        return code;
    }

    // --- 4分の1地域メッシュの計算 ---
    let (t, e_rem) = ((d_rem / 7.5).floor(), d_rem % 7.5);
    let (y, j_rem) = ((i_rem / 11.25).floor(), i_rem % 11.25);
    let n = (t * 2.0) + y + 1.0;
    code.push_str(&(n as u32).to_string()); // 計算結果を追記

    if let MeshLevel::Quarter = level {
        return code;
    }

    // --- 8分の1地域メッシュの計算 ---
    let (t2, _) = ((e_rem / 3.75).floor(), e_rem % 3.75);
    let (y2, _) = ((j_rem / 5.625).floor(), j_rem % 5.625);
    let o = (t2 * 2.0) + y2 + 1.0;
    code.push_str(&(o as u32).to_string()); // 計算結果を追記

    // Eighthが最後のレベルなので、そのまま返す
    code
}
