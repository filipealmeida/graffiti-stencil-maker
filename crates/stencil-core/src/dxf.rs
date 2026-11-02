//! Minimal ASCII DXF (R12) writer: closed polylines on named layers, readable by every CAD and cutter program.
use crate::trace::Pt;

pub struct Layer<'a> {
    pub name: &'a str,
    /// AutoCAD colour index (1 red, 5 blue, 7 white or black).
    pub colour: u8,
    pub polys: &'a [Vec<Pt>],
}

/// One document with the given layers. `y` is written as given (DXF has y pointing up).
pub fn write(layers: &[Layer]) -> String {
    let mut d = String::from("0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1009\n0\nENDSEC\n0\nSECTION\n2\nTABLES\n0\nTABLE\n2\nLAYER\n70\n");
    d += &format!("{}\n", layers.len());
    for l in layers {
        d += &format!(
            "0\nLAYER\n2\n{}\n70\n0\n62\n{}\n6\nCONTINUOUS\n",
            l.name, l.colour
        );
    }
    d += "0\nENDTAB\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n";
    for l in layers {
        for poly in l.polys.iter().filter(|p| p.len() >= 3) {
            d += &format!("0\nPOLYLINE\n8\n{}\n66\n1\n70\n1\n", l.name);
            for p in poly {
                d += &format!(
                    "0\nVERTEX\n8\n{}\n10\n{:.4}\n20\n{:.4}\n30\n0.0\n",
                    l.name, p[0], p[1]
                );
            }
            d += &format!("0\nSEQEND\n8\n{}\n", l.name);
        }
    }
    d += "0\nENDSEC\n0\nEOF\n";
    d
}
