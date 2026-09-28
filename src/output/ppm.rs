use std::{
    fs::{self, File},
    io::{self, BufWriter, Write},
    path::Path,
};

use crate::render::Framebuffer;

pub fn write_ppm(path: &Path, framebuffer: &Framebuffer) -> io::Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }

    let file = File::create(path)?;
    let mut writer = BufWriter::new(file);
    write!(
        writer,
        "P6\n{} {}\n255\n",
        framebuffer.width(),
        framebuffer.height()
    )?;

    for pixel in framebuffer.pixels() {
        writer.write_all(&pixel.to_rgb8())?;
    }

    writer.flush()
}

#[cfg(test)]
mod tests {
    use std::{fs, process};

    use super::write_ppm;
    use crate::render::{Color, Framebuffer};

    #[test]
    fn writes_binary_ppm_header_and_rgb_data() {
        let path = std::env::temp_dir().join(format!("minecraft_diorama_{}.ppm", process::id()));
        let mut framebuffer = Framebuffer::try_new(2, 1).unwrap();
        framebuffer.set_pixel(0, 0, Color::new(1.0, 0.0, 0.0));
        framebuffer.set_pixel(1, 0, Color::new(0.0, 1.0, 0.0));

        write_ppm(&path, &framebuffer).unwrap();
        let bytes = fs::read(&path).unwrap();
        fs::remove_file(&path).unwrap();

        assert_eq!(&bytes[..11], b"P6\n2 1\n255\n");
        assert_eq!(&bytes[11..], &[255, 0, 0, 0, 255, 0]);
    }
}
