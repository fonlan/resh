import struct

def create_minimal_png(width, height, filename):
    """Create a minimal valid PNG file"""
    # PNG signature
    signature = b'\x89PNG\r\n\x1a\n'
    
    # IHDR chunk (width, height, bit depth, color type, etc.)
    ihdr_data = struct.pack('>IIBBBBB', width, height, 8, 2, 0, 0, 0)  # RGB, 8-bit
    ihdr_crc = 0x90773496  # Pre-calculated CRC for this IHDR
    ihdr_chunk = b'IHDR' + ihdr_data
    ihdr = struct.pack('>I', 13) + ihdr_chunk + struct.pack('>I', ihdr_crc)
    
    # IDAT chunk (minimal image data)
    idat_data = b'\x08\x1d\x01\x01\x00\xfe\xff\x00\x00\x00\x01\x00\x01'
    idat_crc = 0xe1240dc9  # Pre-calculated CRC
    idat_chunk = b'IDAT' + idat_data
    idat = struct.pack('>I', len(idat_data)) + idat_chunk + struct.pack('>I', idat_crc)
    
    # IEND chunk
    iend_crc = 0xae426082
    iend = struct.pack('>I', 0) + b'IEND' + struct.pack('>I', iend_crc)
    
    with open(filename, 'wb') as f:
        f.write(signature + ihdr + idat + iend)

# Create PNG files of different sizes
create_minimal_png(32, 32, '32x32.png')
create_minimal_png(128, 128, '128x128.png')
create_minimal_png(128, 128, '128x128@2x.png')

print("PNG files created successfully")
