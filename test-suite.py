#!/usr/bin/env python3
"""
Test Suite for unxml-rs

This script runs the unxml tool on all XML and HTML files in the sample-output directory
and compares the results to detect any changes in output formatting.
"""

import os
import sys
import subprocess
import json
import hashlib
import time
from pathlib import Path
from typing import Dict, List, Tuple, Optional

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
    def __init__(self, sample_dir: str = "sample-output", baseline_file: str = "test-baseline.json", output_dir: str = "expected-output"):
        self.sample_dir = Path(sample_dir)
        self.baseline_file = Path(baseline_file)
        self.output_dir = Path(output_dir)
        self.results = {}
        self.failed_files = []
        self.changed_files = []
        self.new_files = []
        
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
    
    def save_output_file(self, input_file: Path, stdout: str) -> None:
        """Save the stdout to the expected output file"""
        output_file = self.get_expected_output_file(input_file)
        try:
            with open(output_file, 'w', encoding='utf-8') as f:
                f.write(stdout)
        except IOError as e:
            print(f"{Colors.YELLOW}Warning: Could not save output file {output_file}: {e}{Colors.RESET}")
    
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
    
    def run_unxml(self, file_path: Path) -> Tuple[str, str, int]:
        """
        Run the unxml tool on a file and return stdout, stderr, and return code
        """
        try:
            # Build the unxml command - assuming it's built in target/release or target/debug
            # Handle both Unix and Windows executables
            unxml_cmd = None
            if Path("target/release/unxml.exe").exists():
                unxml_cmd = ["target/release/unxml.exe"]
            elif Path("target/release/unxml").exists():
                unxml_cmd = ["target/release/unxml"]
            elif Path("target/debug/unxml.exe").exists():
                unxml_cmd = ["target/debug/unxml.exe"]
            elif Path("target/debug/unxml").exists():
                unxml_cmd = ["target/debug/unxml"]
            else:
                # Try to find it in PATH or build it
                try:
                    subprocess.run(["cargo", "build", "--release"], check=True, capture_output=True)
                    if Path("target/release/unxml.exe").exists():
                        unxml_cmd = ["target/release/unxml.exe"]
                    elif Path("target/release/unxml").exists():
                        unxml_cmd = ["target/release/unxml"]
                except subprocess.CalledProcessError:
                    # Fallback to debug build
                    try:
                        subprocess.run(["cargo", "build"], check=True, capture_output=True)
                        if Path("target/debug/unxml.exe").exists():
                            unxml_cmd = ["target/debug/unxml.exe"]
                        elif Path("target/debug/unxml").exists():
                            unxml_cmd = ["target/debug/unxml"]
                    except subprocess.CalledProcessError:
                        raise RuntimeError("Failed to build unxml binary")
            
            if not unxml_cmd:
                raise RuntimeError("Could not find or build unxml binary")
            
            # Add the file path as argument
            cmd = unxml_cmd + [str(file_path)]
            
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
    
    def calculate_hash(self, content: str) -> str:
        """Calculate SHA-256 hash of content"""
        return hashlib.sha256(content.encode('utf-8')).hexdigest()
    
    def load_baseline(self) -> Dict:
        """Load the baseline results from file"""
        if not self.baseline_file.exists():
            return {}
        
        try:
            with open(self.baseline_file, 'r', encoding='utf-8') as f:
                return json.load(f)
        except (json.JSONDecodeError, IOError) as e:
            print(f"{Colors.YELLOW}Warning: Could not load baseline file: {e}{Colors.RESET}")
            return {}
    
    def save_baseline(self, results: Dict) -> None:
        """Save the current results as baseline"""
        try:
            with open(self.baseline_file, 'w', encoding='utf-8') as f:
                json.dump(results, f, indent=2, sort_keys=True)
        except IOError as e:
            print(f"{Colors.RED}Error: Could not save baseline file: {e}{Colors.RESET}")
    
    def run_tests(self) -> bool:
        """Run tests on all files and compare with baseline"""
        print(f"{Colors.BOLD}Running unxml test suite...{Colors.RESET}")
        print(f"Sample directory: {self.sample_dir}")
        print(f"Expected output directory: {self.output_dir}")
        print(f"Baseline file: {self.baseline_file}")
        print("-" * 60)
        
        test_files = self.find_test_files()
        if not test_files:
            print(f"{Colors.YELLOW}No XML or HTML files found in {self.sample_dir}{Colors.RESET}")
            return True
        
        print(f"Found {len(test_files)} files to test:")
        for file in test_files:
            print(f"  - {file.name}")
        print()
        
        baseline = self.load_baseline()
        current_results = {}
        
        # Run tests on each file
        for file_path in test_files:
            print(f"Testing {file_path.name}... ", end="", flush=True)
            
            stdout, stderr, returncode = self.run_unxml(file_path)
            
            # Save current output to .txt file
            self.save_output_file(file_path, stdout)
            
            # Create result record
            result = {
                'stdout': stdout,
                'stderr': stderr,
                'returncode': returncode,
                'stdout_hash': self.calculate_hash(stdout),
                'stderr_hash': self.calculate_hash(stderr),
                'timestamp': time.time()
            }
            
            current_results[str(file_path)] = result
            
            # Load expected output from .txt file
            expected_output = self.load_expected_output(file_path)
            
            # Compare with baseline and expected output
            file_key = str(file_path)
            if file_key in baseline and expected_output is not None:
                baseline_result = baseline[file_key]
                
                # Check if output changed (compare both baseline and expected output file)
                output_changed = (stdout != expected_output or 
                                result['stderr_hash'] != baseline_result.get('stderr_hash') or
                                result['returncode'] != baseline_result.get('returncode'))
                
                if output_changed:
                    self.changed_files.append(file_path)
                    print(f"{Colors.YELLOW}CHANGED{Colors.RESET}")
                else:
                    print(f"{Colors.GREEN}PASS{Colors.RESET}")
            else:
                self.new_files.append(file_path)
                print(f"{Colors.CYAN}NEW{Colors.RESET}")
            
            # Track failed files (non-zero return code)
            if returncode != 0:
                self.failed_files.append(file_path)
        
        self.results = current_results
        
        # Print summary
        print("\n" + "=" * 60)
        print(f"{Colors.BOLD}Test Summary:{Colors.RESET}")
        print(f"Total files tested: {len(test_files)}")
        print(f"New files: {len(self.new_files)}")
        print(f"Changed files: {len(self.changed_files)}")
        print(f"Failed files: {len(self.failed_files)}")
        print(f"Output files saved to: {self.output_dir}/")
        print(f"  (e.g., {self.output_dir}/simple.xml.txt)")
        
        # Show details for changed files
        if self.changed_files:
            print(f"\n{Colors.YELLOW}Changed files:{Colors.RESET}")
            for file_path in self.changed_files:
                print(f"  - {file_path.name}")
                self.show_file_diff(file_path, baseline, current_results)
        
        # Show details for failed files
        if self.failed_files:
            print(f"\n{Colors.RED}Failed files:{Colors.RESET}")
            for file_path in self.failed_files:
                file_key = str(file_path)
                result = current_results[file_key]
                print(f"  - {file_path.name} (exit code: {result['returncode']})")
                if result['stderr']:
                    print(f"    Error: {result['stderr'][:200]}...")
        
        # Show details for new files
        if self.new_files:
            print(f"\n{Colors.CYAN}New files:{Colors.RESET}")
            for file_path in self.new_files:
                print(f"  - {file_path.name}")
        
        print("\n" + "=" * 60)
        
        # Return True if no changes detected (all tests passed)
        return len(self.changed_files) == 0
    
    def show_file_diff(self, file_path: Path, baseline: Dict, current: Dict) -> None:
        """Show differences for a changed file"""
        file_key = str(file_path)
        baseline_result = baseline.get(file_key, {})
        current_result = current.get(file_key, {})
        expected_output_file = self.get_expected_output_file(file_path)
        
        print(f"    Changes in {file_path.name}:")
        
        if baseline_result.get('returncode') != current_result.get('returncode'):
            print(f"      Return code: {baseline_result.get('returncode', 'N/A')} → {current_result.get('returncode', 'N/A')}")
        
        # Compare with expected output file
        expected_output = self.load_expected_output(file_path)
        current_output = current_result.get('stdout', '')
        
        if expected_output is not None and current_output != expected_output:
            print(f"      Output differs from expected output file: {expected_output_file}")
            print(f"        Expected hash: {self.calculate_hash(expected_output)[:8]}...")
            print(f"        Current hash:  {self.calculate_hash(current_output)[:8]}...")
            print(f"        Use 'diff {expected_output_file} <(unxml {file_path})' to see differences")
        elif baseline_result.get('stdout_hash') != current_result.get('stdout_hash'):
            print(f"      Output changed (hash: {baseline_result.get('stdout_hash', 'N/A')[:8]}... → {current_result.get('stdout_hash', 'N/A')[:8]}...)")
        
        if baseline_result.get('stderr_hash') != current_result.get('stderr_hash'):
            print(f"      Error output changed (hash: {baseline_result.get('stderr_hash', 'N/A')[:8]}... → {current_result.get('stderr_hash', 'N/A')[:8]}...)")
    
    def update_baseline(self) -> None:
        """Update the baseline with current results"""
        if self.results:
            self.save_baseline(self.results)
            print(f"{Colors.GREEN}Baseline updated with current results.{Colors.RESET}")
        else:
            print(f"{Colors.YELLOW}No results to save.{Colors.RESET}")
    
    def show_detailed_output(self, file_name: str) -> None:
        """Show detailed output for a specific file"""
        file_path = self.sample_dir / file_name
        if not file_path.exists():
            print(f"{Colors.RED}File not found: {file_name}{Colors.RESET}")
            return
        
        file_key = str(file_path)
        if file_key not in self.results:
            print(f"{Colors.YELLOW}No test results for: {file_name}{Colors.RESET}")
            return
        
        result = self.results[file_key]
        print(f"\n{Colors.BOLD}Detailed output for {file_name}:{Colors.RESET}")
        print(f"Return code: {result['returncode']}")
        print(f"Stdout hash: {result['stdout_hash']}")
        print(f"Stderr hash: {result['stderr_hash']}")
        
        if result['stdout']:
            print(f"\n{Colors.CYAN}STDOUT:{Colors.RESET}")
            print(result['stdout'])
        
        if result['stderr']:
            print(f"\n{Colors.RED}STDERR:{Colors.RESET}")
            print(result['stderr'])

def main():
    """Main function to run the test suite"""
    import argparse
    
    parser = argparse.ArgumentParser(description='Test suite for unxml-rs')
    parser.add_argument('--sample-dir', default='sample-output', 
                       help='Directory containing sample XML/HTML files')
    parser.add_argument('--baseline', default='test-baseline.json',
                       help='Baseline file to store/compare results')
    parser.add_argument('--output-dir', default='expected-output',
                       help='Directory to store expected output .txt files')
    parser.add_argument('--update-baseline', action='store_true',
                       help='Update baseline with current results')
    parser.add_argument('--show-output', type=str, metavar='FILENAME',
                       help='Show detailed output for a specific file')
    parser.add_argument('--build', action='store_true',
                       help='Build the unxml binary before running tests')
    
    args = parser.parse_args()
    
    # Build if requested
    if args.build:
        print("Building unxml...")
        try:
            subprocess.run(["cargo", "build", "--release"], check=True)
            print(f"{Colors.GREEN}Build successful.{Colors.RESET}")
        except subprocess.CalledProcessError as e:
            print(f"{Colors.RED}Build failed: {e}{Colors.RESET}")
            return 1
    
    runner = TestRunner(args.sample_dir, args.baseline, args.output_dir)
    
    if args.show_output:
        runner.run_tests()
        runner.show_detailed_output(args.show_output)
        return 0
    
    # Run the tests
    success = runner.run_tests()
    
    if args.update_baseline:
        runner.update_baseline()
    
    # Return appropriate exit code
    return 0 if success else 1

if __name__ == "__main__":
    sys.exit(main()) 