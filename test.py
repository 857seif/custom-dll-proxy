import tkinter as tk
from tkinter import filedialog, messagebox, scrolledtext
import ctypes
import os

class SimpleDLLLoader:
    def __init__(self, root):
        self.root = root
        self.root.title("DLL Loader")
        self.root.geometry("500x200")
        self.root.resizable(False, False)
        
        self.dll_handle = None
        self.dll_path = None
        
        self.setup_ui()
    
    def setup_ui(self):
        main_frame = tk.Frame(self.root, padx=10, pady=10)
        main_frame.pack(fill=tk.BOTH, expand=True)
        
        top_frame = tk.Frame(main_frame)
        top_frame.pack(fill=tk.X, pady=(0, 10))
        
        self.path_var = tk.StringVar()
        self.path_entry = tk.Entry(top_frame, textvariable=self.path_var, width=50)
        self.path_entry.pack(side=tk.LEFT, fill=tk.X, expand=True, padx=(0, 5))
        
        browse_btn = tk.Button(top_frame, text="Browse", command=self.browse_dll)
        browse_btn.pack(side=tk.LEFT, padx=(0, 5))
        
        load_btn = tk.Button(top_frame, text="Load", command=self.load_dll, bg="#4CAF50", fg="white")
        load_btn.pack(side=tk.LEFT)
        
        self.output_text = scrolledtext.ScrolledText(main_frame, wrap=tk.WORD, height=5)
        self.output_text.pack(fill=tk.BOTH, expand=True)
        
        self.status_var = tk.StringVar(value="Ready")
        self.status_label = tk.Label(main_frame, textvariable=self.status_var, 
                                     relief=tk.SUNKEN, anchor=tk.W)
        self.status_label.pack(fill=tk.X, pady=(10, 0))
    
    def browse_dll(self):
        file_path = filedialog.askopenfilename(
            title="Select DLL File",
            filetypes=[("DLL Files", "*.dll"), ("All Files", "*.*")]
        )
        if file_path:
            self.path_var.set(file_path)
            self.dll_path = file_path
            self.log(f"Selected: {os.path.basename(file_path)}")
    
    def load_dll(self):
        dll_path = self.path_var.get().strip()
        
        if not dll_path:
            messagebox.showerror("Error", "Please select a DLL file!")
            return
        
        if not os.path.exists(dll_path):
            messagebox.showerror("Error", f"File not found: {dll_path}")
            return
        
        try:
            self.log(f"Loading: {os.path.basename(dll_path)}...")
            self.status_var.set("Loading...")
            self.root.update()
            
            self.dll_handle = ctypes.WinDLL(dll_path)
            
            self.log("SUCCESS: DLL loaded successfully!")
            self.status_var.set(f"Loaded: {os.path.basename(dll_path)}")
            
        except Exception as e:
            error_msg = f"Failed to load DLL: {str(e)}"
            self.log(f"ERROR: {error_msg}")
            self.status_var.set("Load failed")
            messagebox.showerror("Error", error_msg)
    
    def log(self, message):
        self.output_text.insert(tk.END, message + "\n")
        self.output_text.see(tk.END)

def main():
    root = tk.Tk()
    app = SimpleDLLLoader(root)
    root.mainloop()

if __name__ == "__main__":
    main()