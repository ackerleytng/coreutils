#![crate_name = "uu_od"]

/*
 * This file is part of the uutils coreutils package.
 *
 * (c) Ben Hirsch <benhirsch24@gmail.com>
 *
 * For the full copyright and license information, please view the LICENSE
 * file that was distributed with this source code.
 */

extern crate getopts;
extern crate unindent;
extern crate byteorder;

mod multifilereader;
mod prn_int;
mod prn_char;
mod prn_float;

use std::f64;
use unindent::*;
use byteorder::*;
use multifilereader::*;
use prn_int::*;
use prn_char::*;
use prn_float::*;

//This is available in some versions of std, but not all that we target.
macro_rules! hashmap {
    ($( $key: expr => $val: expr ),*) => {{
         let mut map = ::std::collections::HashMap::new();
         $( map.insert($key, $val); )*
         map
    }}
}

static NAME: &'static str = "od";
static VERSION: &'static str = env!("CARGO_PKG_VERSION");

#[derive(Debug)]
enum Radix { Decimal, Hexadecimal, Octal, Binary }

pub fn uumain(args: Vec<String>) -> i32 {
    let mut opts = getopts::Options::new();

    opts.optopt("A", "address-radix",
                "Select the base in which file offsets are printed.", "RADIX");
    opts.optopt("j", "skip-bytes",
                "Skip bytes input bytes before formatting and writing.", "BYTES");
    opts.optopt("N", "read-bytes",
                "limit dump to BYTES input bytes", "BYTES");
    opts.optopt("S", "strings",
                ("output strings of at least BYTES graphic chars. 3 is assumed when \
                 BYTES is not specified."),
                "BYTES");
    opts.optflag("a", "", "named characters, ignoring high-order bit");
    opts.optflag("b", "", "octal bytes");
    opts.optflag("c", "", "ASCII characters or backslash escapes");
    opts.optflag("d", "", "unsigned decimal 2-byte units");
    opts.optflag("o", "", "unsigned decimal 2-byte units");

    opts.optflag("I", "", "decimal 2-byte units");
    opts.optflag("L", "", "decimal 2-byte units");
    opts.optflag("i", "", "decimal 2-byte units");

    opts.optflag("O", "", "octal 4-byte units");
    opts.optflag("s", "", "decimal 4-byte units");

    opts.optflag("e", "", "floating point double precision (64-bit) units");
    opts.optflag("f", "", "floating point single precision (32-bit) units");
    opts.optflag("F", "", "floating point double precision (64-bit) units");

    opts.optopt("t", "format", "select output format or formats", "TYPE");
    opts.optflag("v", "output-duplicates", "do not use * to mark line suppression");
    opts.optopt("w", "width",
                ("output BYTES bytes per output line. 32 is implied when BYTES is not \
                 specified."),
                "BYTES");
    opts.optflag("h", "help", "display this help and exit.");
    opts.optflag("", "version", "output version information and exit.");

    let matches = match opts.parse(&args[1..]) {
        Ok(m) => m,
        Err(f) => panic!("Invalid options\n{}", f)
    };

    if matches.opt_present("h") {
        let msg = unindent(&format!("
                Usage:
                    {0} [OPTION]... [FILENAME]...

                Displays data in various human-readable formats.", NAME));
        println!("{}", opts.usage(&msg));
        return 0;
    }
    if matches.opt_present("version") {
        println!("{} {}", NAME, VERSION);
        return 0;
    }

    let input_offset_base = match parse_radix(matches.opt_str("A")) {
        Ok(r) => r,
        Err(f) => { panic!("Invalid -A/--address-radix\n{}", f) }
    };

    // Gather up file names - args which don't start with '-'
    let stdnionly = [InputSource::Stdin];
    let inputs = args[1..]
        .iter()
        .filter_map(|w| match w as &str {
            "--" => Some(InputSource::Stdin),
            o if o.starts_with("-") => None,
            x => Some(InputSource::FileName(x)),
        })
        .collect::<Vec<_>>();
    // If no input files named, use stdin.
    let inputs = if inputs.len() == 0 {
        &stdnionly[..]
    } else {
        &inputs[..]
    };
    // Gather up format flags, we don't use getopts becase we need keep them in order.
    let flags = args[1..]
        .iter()
        .filter_map(|w| match w as &str {
            "--" => None,
            o if o.starts_with("-") => Some(&o[1..]),
            _ => None,
        })
        .collect::<Vec<_>>();

        // At the moment, char (-a & -c)formats need the driver to set up a
        // line by inserting a different # of of spaces at the start.
        struct OdFormater {
            writer: FormatWriter,
            offmarg: usize,
        };
        let oct = OdFormater {
            writer: FormatWriter::IntWriter(print_item_oct), offmarg: 2
        };
        let hex = OdFormater {
            writer: FormatWriter::IntWriter(print_item_hex), offmarg: 2
        };
        let dec_u = OdFormater {
            writer: FormatWriter::IntWriter(print_item_dec_u), offmarg: 2
        };
        let dec_s = OdFormater {
            writer: FormatWriter::IntWriter(print_item_dec_s), offmarg: 2
        };
        let a_char = OdFormater {
            writer: FormatWriter::IntWriter(print_item_a), offmarg: 1
        };
        let c_char = OdFormater {
            writer: FormatWriter::IntWriter(print_item_c), offmarg: 1
        };
        let flo32 = OdFormater {
            writer: FormatWriter::FloatWriter(print_item_flo32), offmarg: 0
        };
        let flo64 = OdFormater {
            writer: FormatWriter::FloatWriter(print_item_flo64), offmarg: 0
        };

        fn mkfmt(itembytes: usize, fmtspec: &OdFormater) -> OdFormat {
            OdFormat {
                itembytes: itembytes,
                writer: fmtspec.writer,
                offmarg: fmtspec.offmarg,
            }
        }

// TODO: -t fmts
        let known_formats = hashmap![
    		"a" => (1, &a_char),
    		"B" => (2, &oct) ,
    		"b" => (1, &oct),
    		"c" => (1, &c_char),
    		"D" => (4, &dec_u),
    		"e" => (8, &flo64),
    		"F" => (8, &flo64),
    		"f" => (4, &flo32),
    		"H" => (4, &hex),
    		"X" => (4, &hex) ,
    		"o" => (2, &oct),
    		"x" => (2, &hex),
    		"h" => (2, &hex),

    		"I" => (2, &dec_s),
    		"L" => (2, &dec_s),
    		"i" => (2, &dec_s),

    		"O" => (4, &oct),
    		"s" => (2, &dec_u)
    	];

        let mut formats = Vec::new();

        for flag in flags.iter() {
            match known_formats.get(flag) {
                None => {} // not every option is a format
                Some(r) => {
                    let (itembytes, fmtspec) = *r;
                    formats.push(mkfmt(itembytes, fmtspec))
                }
            }
        }

        if formats.is_empty() {
            formats.push(mkfmt(2, &oct)); // 2 byte octal is the default
        }

        odfunc(&input_offset_base, &inputs, &formats[..])
}

const LINEBYTES:usize = 16;
const WORDBYTES:usize = 2;

fn odfunc(input_offset_base: &Radix, fnames: &[InputSource], formats: &[OdFormat]) -> i32 {

    let mut mf = MultifileReader::new(fnames);
    let mut addr = 0;
    let bytes = &mut [b'\x00'; LINEBYTES];
    loop {
        // print each line data (or multi-format raster of several lines describing the same data).

        print_with_radix(input_offset_base, addr); // print offset
		// if printing in multiple formats offset is printed only once

        match mf.f_read(bytes) {
            Ok(0) => {
                print!("\n");
                break;
            }
            Ok(n) => {
                let mut first = true; // First line of a multi-format raster.
                for f in formats {
                    if !first {
                        // this takes the space of the file offset on subsequent
                        // lines of multi-format rasters.
                        print!("       ");
                    }
                    first = false;
                    print!("{:>width$}", "", width = f.offmarg);// 4 spaces after offset - we print 2 more before each word

                    // not enough byte for a whole element, this should only happen on the last line.
                    if n % f.itembytes != 0 {
                        let b = n / f.itembytes;
                        // set zero bytes in the part of the buffer that will be used, but is not filled.
                        for i in n..(b + 1) * f.itembytes {
                            bytes[i] = 0;
                        }
                    }

                    let mut b = 0;
                    while b < n {
                        let nextb = b + f.itembytes;
                        match f.writer {
                            FormatWriter::IntWriter(func) => {
                                let p: u64 = match f.itembytes {
                                    1 => {
                                        bytes[b] as u64
                                    }
                                    2 => {
                                        LittleEndian::read_u16(&bytes[b..nextb]) as u64
                                    }
                                    4 => {
                                        LittleEndian::read_u32(&bytes[b..nextb]) as u64
                                    }
                                    8 => {
                                        LittleEndian::read_u64(&bytes[b..nextb])
                                    }
                                    _ => { panic!("Invalid itembytes: {}", f.itembytes); }
                                };
                                func(p, f.itembytes);
                            }
                            FormatWriter::FloatWriter(func) => {
                                let p: f64 = match f.itembytes {
                                    4 => {
                                        LittleEndian::read_f32(&bytes[b..nextb]) as f64
                                    }
                                    8 => {
                                        LittleEndian::read_f64(&bytes[b..nextb])
                                    }
                                    _ => { panic!("Invalid itembytes: {}", f.itembytes); }
                                };
                                func(p);
                            }
                        }
                        b = nextb;
                    }
                    // Add extra spaces to pad out the short, presumably last, line.
                    if n < LINEBYTES {
                        // calc # of items we did not print, must be short at least WORDBYTES to be missing any.
                        let words_short = (LINEBYTES - n) / WORDBYTES;
                        // XXX this is running short for -c & -a
                        print!("{:>width$}", "", width = (words_short) * (6 + 2));
                    }
                    print!("\n");
                }
                addr += n;
            }
            Err(_) => {
                break;
            }
        };
    }
    if mf.any_err {
        1
    } else {
        0
    }
}

// For file byte offset printed at left margin.
fn parse_radix(radix_str: Option<String>) -> Result<Radix, &'static str> {
    match radix_str {
        None => Ok(Radix::Octal),
        Some(s) => {
            let st = s.into_bytes();
            if st.len() != 1 {
                Err("Radix must be one of [d, o, b, x]\n")
            } else {
                let radix: char = *(st.get(0)
                                      .expect("byte string of length 1 lacks a 0th elem")) as char;
                match radix {
                    'd' => Ok(Radix::Decimal),
                    'x' => Ok(Radix::Hexadecimal),
                    'o' => Ok(Radix::Octal),
                    'b' => Ok(Radix::Binary),
                    _ => Err("Radix must be one of [d, o, b, x]\n")
                }
            }
        }
    }
}

fn print_with_radix(r: &Radix, x: usize) {
    // TODO(keunwoo): field widths should be based on sizeof(x), or chosen dynamically based on the
    // expected range of address values.  Binary in particular is not great here.
    match *r {
        Radix::Decimal => print!("{:07}", x),
        Radix::Hexadecimal => print!("{:07X}", x),
        Radix::Octal => print!("{:07o}", x),
        Radix::Binary => print!("{:07b}", x)
    }
}

#[derive(Clone, Copy)]
enum FormatWriter {
    IntWriter(fn(u64, usize)),
    FloatWriter(fn(f64)),
}

struct OdFormat {
    itembytes: usize,
    writer: FormatWriter,
    offmarg: usize,
}
