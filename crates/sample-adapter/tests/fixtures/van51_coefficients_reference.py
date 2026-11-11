#!/usr/bin/env python3
"""Regenerate the Van51 coefficient modules and isolated native goldens.

ARM64 register/lane replay is authoritative for the polynomial coefficients:
the saved Ghidra pseudocode aliases overwritten SIMD accumulator locals and
corrupts four feedback expressions. Native instruction execution supplies the
independent numerical golden, including all seven polynomial control inputs.
"""
from pathlib import Path
import argparse,hashlib,json,subprocess,tempfile
from van51_coefficient_assembly import BINARY_PATH, rust_helper
from van51_native_coefficients_reference import main as native_goldens
ROOT=Path(__file__).resolve().parents[4]
HELPERS=[('tone','van51_tone_stack.rs','0x1005cf4c8','0x1005d3540'),('lead','van51_lead_filter.rs','0x1005d3d2c','0x1005d4fe0'),('feedback','van51_feedback_filter.rs','0x1005d3540','0x1005d3d2c')]
def main():
 parser=argparse.ArgumentParser();parser.add_argument('--rust',action='store_true');args=parser.parse_args()
 if args.rust:
  with tempfile.TemporaryDirectory() as temp:
   for kind,filename,start,end in HELPERS:
    assembly=Path(temp)/(kind+'.asm')
    result=subprocess.run(['/opt/homebrew/opt/llvm/bin/llvm-objdump','-d','--start-address='+start,'--stop-address='+end,str(BINARY_PATH)],check=True,text=True,capture_output=True)
    assembly.write_text(result.stdout)
    output=ROOT/'crates/sample-adapter/src'/filename
    output.write_text(rust_helper(str(assembly),kind))
    subprocess.run(['rustfmt','--edition','2021',str(output)],check=True)
 native_goldens()
if __name__=='__main__':main()
