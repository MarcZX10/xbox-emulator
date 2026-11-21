# usage: perl apply.pl <target file> <patch file>
# patch file: blocks of  "@@OLD\n...\n@@NEW\n...\n@@END\n"; each OLD must occur literally.
# Atomic: the target is only written if every block applies.
use strict; use warnings;
my ($f, $p) = @ARGV;
local $/;
open my $in, "<", $f or die "open $f"; binmode $in; my $s = <$in>; close $in;
open my $pf, "<", $p or die "open $p"; binmode $pf; my $t = <$pf>; close $pf;
$t =~ tr/\r//d;
my $crlf = ($s =~ /\r\n/) ? 1 : 0;
$s =~ tr/\r//d;
my $n = 0;
while ($t =~ /\@\@OLD\n(.*?)\n\@\@NEW\n(.*?)\n\@\@END\n/sg) {
    my ($old, $new) = ($1, $2);
    my $i = index($s, $old);
    die "NOT FOUND (block " . ($n + 1) . "): " . substr($old, 0, 100) . "\n" if $i < 0;
    substr($s, $i, length($old)) = $new; $n++;
}
$s =~ s/\n/\r\n/g if $crlf;
open my $out, ">", $f or die; binmode $out; print $out $s; close $out;
print "applied $n replacements to $f\n";
