import os
import sys
from rembg import remove

def process_image(input_path):
    # Output always as PNG to support transparency
    name, ext = os.path.splitext(input_path)
    if input_path.endswith('_transparent.png'):
        return # Skip already processed files
        
    output_path = f"{name}_transparent.png"
    
    try:
        print(f"Processing: {os.path.basename(input_path)}")
        with open(input_path, 'rb') as i:
            input_data = i.read()
            
        output_data = remove(input_data)
        
        with open(output_path, 'wb') as o:
            o.write(output_data)
        print(f"  -> Saved to {os.path.basename(output_path)}")
        
    except Exception as e:
        print(f"  -> Failed to process {os.path.basename(input_path)}: {e}")

if __name__ == "__main__":
    # Define directories where icons/emotes exist
    target_dirs = [
        "client/src/assets/icons",
        "client/src/assets/images",
        "client/src/assets/emotes",
        "client/src/assets"
    ]
    
    for directory in target_dirs:
        if not os.path.exists(directory):
            print(f"Skipping {directory} (not found)")
            continue
            
        print(f"\nScanning {directory}...")
        for filename in os.listdir(directory):
            if filename.lower().endswith(('.png', '.jpg', '.jpeg', '.webp')):
                process_image(os.path.join(directory, filename))
                
    print("\nDone! Please review the newly generated '_transparent.png' files.")
