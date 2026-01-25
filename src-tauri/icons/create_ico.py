import struct

def create_minimal_ico(filename):
    """Create a minimal valid ICO file"""
    width, height = 32, 32
    
    # ICO header (6 bytes): Reserved (2) + Type (2) + Count (2)
    ico_header = struct.pack('<HHH', 0, 1, 1)
    
    # Image data (blank 32x32 RGBA)
    image_data = b'\x00' * (width * height * 4)
    
    # BMP info header (40 bytes)
    bmp_header = struct.pack('<IHHHHHIIHHHII',
        40,             # Header size
        width,          # Width
        height * 2,     # Height (doubled for ICO format)
        1, 32,          # Planes, Bits per pixel
        0,              # Compression
        len(image_data),  # Image size
        0, 0, 0, 0      # DPI/colors (not used)
    )
    
    # Directory entry (16 bytes)
    image_size = len(bmp_header) + len(image_data)
    dir_entry = struct.pack('<BBBBHHI',
        width, height,  # Width, Height
        0, 0,           # Color count, Reserved
        1, 32,          # Planes, Bits per pixel
        image_size      # Size
    )
    
    # Offset is 6 (ico header) + 16 (dir entry) = 22
    dir_entry = dir_entry + struct.pack('<I', 22)
    
    with open(filename, 'wb') as f:
        f.write(ico_header)
        f.write(dir_entry)
        f.write(bmp_header)
        f.write(image_data)

create_minimal_ico('icon.ico')
print("ICO file created successfully")
