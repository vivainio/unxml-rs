#!/usr/bin/env python3
"""
Test Suite for unxml-rs

This script runs the unxml tool on all XML and HTML files in the test-input directory
and compares the results with expected output files.
"""

import os
import sys
import subprocess
import time
from pathlib import Path
from typing import List, Tuple, Optional

class Colors:
    """ANSI color codes for terminal output"""
    GREEN = '\033[92m'
    RED = '\033[91m'
    YELLOW = '\033[93m'
    BLUE = '\033[94m'
    MAGENTA = '\033[95m'
    CYAN = '\033[96m'
    WHITE = '\033[97m'
    BOLD = '\033[1m'
    RESET = '\033[0m'

class TestRunner:
    def __init__(self, sample_dir: str = "test-input", output_dir: str = "expected-output", update_mode: bool = False):
        self.sample_dir = Path(sample_dir)
        self.output_dir = Path(output_dir)
        self.update_mode = update_mode
        self.failed_files = []
        self.changed_files = []
        self.new_files = []
        self.passed_files = []
        self.updated_files = []
        self.unxml_cmd = None  # Will be set after building the binary
        
        # Ensure sample directory exists
        if not self.sample_dir.exists():
            print(f"{Colors.RED}Error: Sample directory '{sample_dir}' does not exist!{Colors.RESET}")
            sys.exit(1)
        
        # Create output directory if it doesn't exist
        self.output_dir.mkdir(exist_ok=True)
    
    def find_test_files(self) -> List[Path]:
        """Find all XML and HTML files in the sample directory"""
        extensions = ['*.xml', '*.html', '*.htm']
        files = []
        
        for ext in extensions:
            files.extend(self.sample_dir.glob(ext))
        
        return sorted(files)
    
    def get_expected_output_file(self, input_file: Path) -> Path:
        """Get the expected output file path for a given input file"""
        return self.output_dir / f"{input_file.name}.txt"
    
    def load_expected_output(self, input_file: Path) -> Optional[str]:
        """Load the expected output for a given input file"""
        output_file = self.get_expected_output_file(input_file)
        if not output_file.exists():
            return None
        
        try:
            with open(output_file, 'r', encoding='utf-8') as f:
                return f.read()
        except IOError as e:
            print(f"{Colors.YELLOW}Warning: Could not load expected output file {output_file}: {e}{Colors.RESET}")
            return None
    
    def save_expected_output(self, input_file: Path, output: str) -> bool:
        """Save the expected output for a given input file"""
        output_file = self.get_expected_output_file(input_file)
        
        try:
            with open(output_file, 'w', encoding='utf-8') as f:
                f.write(output)
            return True
        except IOError as e:
            print(f"{Colors.RED}Error: Could not save expected output file {output_file}: {e}{Colors.RESET}")
            return False
    
    def build_binary(self) -> None:
        """Build a fresh binary to ensure we're testing the latest code"""
        print(f"{Colors.BOLD}Building fresh binary...{Colors.RESET}", end=" ", flush=True)
        
        # First try to build release version
        try:
            result = subprocess.run(["cargo", "build", "--release"], check=True, capture_output=True)
            print(f"{Colors.GREEN}✓ (release){Colors.RESET}")
            if Path("target/release/unxml.exe").exists():
                self.unxml_cmd = ["target/release/unxml.exe"]
            elif Path("target/release/unxml").exists():
                self.unxml_cmd = ["target/release/unxml"]
        except subprocess.CalledProcessError as e:
            print(f"{Colors.YELLOW}Release failed, trying debug...{Colors.RESET}", end=" ", flush=True)
            # Fallback to debug build
            try:
                subprocess.run(["cargo", "build"], check=True, capture_output=True)
                print(f"{Colors.GREEN}✓ (debug){Colors.RESET}")
                if Path("target/debug/unxml.exe").exists():
                    self.unxml_cmd = ["target/debug/unxml.exe"]
                elif Path("target/debug/unxml").exists():
                    self.unxml_cmd = ["target/debug/unxml"]
            except subprocess.CalledProcessError as debug_error:
                print(f"{Colors.RED}✗{Colors.RESET}")
                release_error_msg = e.stderr.decode() if e.stderr else 'Unknown error'
                debug_error_msg = debug_error.stderr.decode() if debug_error.stderr else 'Unknown error'
                raise RuntimeError(f"Failed to build unxml binary.\nRelease build error: {release_error_msg}\nDebug build error: {debug_error_msg}")
        
        if not self.unxml_cmd:
            raise RuntimeError("Could not find built unxml binary after successful build")
    
    def run_unxml(self, file_path: Path) -> Tuple[str, str, int]:
        """
        Run the unxml tool on a file and return stdout, stderr, and return code
        """
        try:
            # Ensure binary is built
            if not self.unxml_cmd:
                raise RuntimeError("Binary not built. Call build_binary() first.")
            
            # Add the file path as argument
            cmd = self.unxml_cmd + [str(file_path)]
            
            result = subprocess.run(
                cmd,
                capture_output=True,
                text=True,
                timeout=30  # 30 second timeout
            )
            
            return result.stdout, result.stderr, result.returncode
            
        except subprocess.TimeoutExpired:
            return "", "Timeout: Command took longer than 30 seconds", -1
        except Exception as e:
            return "", f"Error running unxml: {str(e)}", -1
    
    def run_tests(self) -> bool:
        """Run tests on all files and compare with expected output"""
        if self.update_mode:
            print(f"{Colors.BOLD}Updating reference outputs...{Colors.RESET}")
        else:
            print(f"{Colors.BOLD}Running unxml test suite...{Colors.RESET}")
        print(f"Sample directory: {self.sample_dir}")
        print(f"Expected output directory: {self.output_dir}")
        print("-" * 60)
        
        # Build fresh binary first
        self.build_binary()
        print()
        
        test_files = self.find_test_files()
        if not test_files:
            print(f"{Colors.YELLOW}No XML or HTML files found in {self.sample_dir}{Colors.RESET}")
            return True
        
        print(f"Found {len(test_files)} files to test:")
        for file in test_files:
            print(f"  - {file.name}")
        print()
        
        # Run tests on each file
        for file_path in test_files:
            if self.update_mode:
                print(f"Updating {file_path.name}... ", end="", flush=True)
            else:
                print(f"Testing {file_path.name}... ", end="", flush=True)
            
            stdout, stderr, returncode = self.run_unxml(file_path)
            
            if returncode != 0:
                # Command failed
                self.failed_files.append((file_path, stderr, returncode))
                print(f"{Colors.RED}FAILED{Colors.RESET}")
                continue
            
            if self.update_mode:
                # Update mode: save output as expected result
                if self.save_expected_output(file_path, stdout):
                    self.updated_files.append(file_path)
                    print(f"{Colors.GREEN}UPDATED{Colors.RESET}")
                else:
                    self.failed_files.append((file_path, "Failed to save expected output", -1))
                    print(f"{Colors.RED}FAILED{Colors.RESET}")
            else:
                # Test mode: compare with expected output
                expected_output = self.load_expected_output(file_path)
                
                if expected_output is None:
                    # No expected output file exists - this is a new file
                    self.new_files.append(file_path)
                    print(f"{Colors.CYAN}NEW{Colors.RESET}")
                elif stdout != expected_output:
                    # Output doesn't match expected
                    self.changed_files.append(file_path)
                    print(f"{Colors.YELLOW}CHANGED{Colors.RESET}")
                else:
                    # All good
                    self.passed_files.append(file_path)
                    print(f"{Colors.GREEN}PASS{Colors.RESET}")
        
        # Print summary
        print("\n" + "=" * 60)
        if self.update_mode:
            print(f"{Colors.BOLD}Update Summary:{Colors.RESET}")
            print(f"Total files processed: {len(test_files)}")
            print(f"Updated: {len(self.updated_files)}")
            print(f"Failed: {len(self.failed_files)}")
        else:
            print(f"{Colors.BOLD}Test Summary:{Colors.RESET}")
            print(f"Total files tested: {len(test_files)}")
            print(f"Passed: {len(self.passed_files)}")
            print(f"New files: {len(self.new_files)}")
            print(f"Changed: {len(self.changed_files)}")
            print(f"Failed: {len(self.failed_files)}")
        
        if self.update_mode:
            # Show details for updated files
            if self.updated_files:
                print(f"\n{Colors.GREEN}Updated files:{Colors.RESET}")
                for file_path in self.updated_files:
                    expected_file = self.get_expected_output_file(file_path)
                    print(f"  - {file_path.name} -> {expected_file}")
        else:
            # Show details for new files
            if self.new_files:
                print(f"\n{Colors.CYAN}New files (no expected output):{Colors.RESET}")
                for file_path in self.new_files:
                    print(f"  - {file_path.name}")
                    expected_file = self.get_expected_output_file(file_path)
                    print(f"    Run: python test-suite.py --update")
            
            # Show details for changed files
            if self.changed_files:
                print(f"\n{Colors.YELLOW}Changed files (output differs from expected):{Colors.RESET}")
                for file_path in self.changed_files:
                    expected_file = self.get_expected_output_file(file_path)
                    print(f"  - {file_path.name}")
                    print(f"    Compare: diff {expected_file} <(unxml {file_path})")
                    print(f"    Update: python test-suite.py --update")
        
        # Show details for failed files
        if self.failed_files:
            print(f"\n{Colors.RED}Failed files:{Colors.RESET}")
            for file_path, stderr, returncode in self.failed_files:
                print(f"  - {file_path.name} (exit code: {returncode})")
                if stderr and isinstance(stderr, str):
                    print(f"    Error: {stderr[:200]}{'...' if len(stderr) > 200 else ''}")
        
        print("\n" + "=" * 60)
        
        # Return True if no failures occurred
        if self.update_mode:
            return len(self.failed_files) == 0
        else:
            return len(self.changed_files) == 0 and len(self.failed_files) == 0

def main():
    """Main function to run the test suite"""
    import argparse
    
    parser = argparse.ArgumentParser(description='Test suite for unxml-rs')
    parser.add_argument('--sample-dir', default='test-input', 
                       help='Directory containing sample XML/HTML files')
    parser.add_argument('--output-dir', default='expected-output',
                       help='Directory containing expected output .txt files')
    parser.add_argument('--update', action='store_true',
                       help='Update expected output files instead of comparing them')
    
    args = parser.parse_args()
    
    runner = TestRunner(args.sample_dir, args.output_dir, args.update)
    
    # Run the tests or update expected outputs
    success = runner.run_tests()
    
    # Return appropriate exit code
    return 0 if success else 1

if __name__ == "__main__":
    sys.exit(main()) 