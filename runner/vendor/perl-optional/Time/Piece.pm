package Time::Piece;
# =============================================================================
#  testphpv2 自带的最小 Time::Piece 实现（纯 perl，不需要编译 XS）
#
#  为什么需要：精简系统可能只装了 perl 解释器，连 perl-core 的 XS 模块都没有。
#  OpenSSL 的 Configure 会 `use Time::Piece;`（只用来格式化 RELEASE_DATE），
#  没有它整个 openssl 就编不了。系统自带真的 Time::Piece 时不会用到本文件
#  （openssl.sh 只在系统加载失败时才把这个目录挂进 PERL5LIB）。
#
#  只实现构建脚本常用的一部分：new/localtime/gmtime/strptime/strftime/epoch
#  以及 year/mon/mday/hour/min/sec/wday/yday/ymd/hms/datetime。
# =============================================================================
use strict;
use warnings;
use Carp ();
use POSIX ();

our $VERSION   = '1.36';
our $DATE_SEP  = '-';
our $TIME_SEP  = ':';
our $IS_WIN32  = ($^O =~ /Win32/);

use Exporter ();
our @ISA       = qw(Exporter);
our @EXPORT    = qw(localtime gmtime);
our %EXPORT_TAGS = (':override' => 'internal');

my @MON_LIST  = qw(Jan Feb Mar Apr May Jun Jul Aug Sep Oct Nov Dec);
my @FULLMON_LIST = qw(January February March April May June July
                      August September October November December);
my @DAY_LIST  = qw(Sun Mon Tue Wed Thu Fri Sat);
my @FULLDAY_LIST = qw(Sunday Monday Tuesday Wednesday Thursday Friday Saturday);
my %MON_NUM = map { lc($MON_LIST[$_]) => $_ + 1 } 0 .. 11;
my %FULLMON_NUM = map { lc($FULLMON_LIST[$_]) => $_ + 1 } 0 .. 11;
my %DAY_NUM = map { lc($DAY_LIST[$_]) => $_ } 0 .. 6;

sub _c { my ($s, $l) = @_; Carp::croak("$s") if $l; }

sub new {
    my ($class, $time) = @_;
    $class = ref($class) || $class || 'Time::Piece';
    $time = time unless defined $time;
    my @lt = localtime($time);
    return bless { _epoch => $time, _islocal => 1 }, $class;
}

sub localtime {
    my $class = shift;
    my $time  = shift;
    $time = time unless defined $time;
    return bless { _epoch => $time, _islocal => 1 }, (ref($class) || $class || 'Time::Piece');
}

sub gmtime {
    my $class = shift;
    my $time  = shift;
    $time = time unless defined $time;
    return bless { _epoch => $time, _islocal => 0 }, (ref($class) || $class || 'Time::Piece');
}

sub _fields {
    my $self = shift;
    my @t = $self->{_islocal} ? CORE::localtime($self->{_epoch})
                              : CORE::gmtime($self->{_epoch});
    return @t;
}

sub epoch { $_[0]->{_epoch} }
sub year  { ($_[0]->_fields)[5] + 1900 }
sub mon   { ($_[0]->_fields)[4] + 1 }
sub mday  { ($_[0]->_fields)[3] }
sub hour  { ($_[0]->_fields)[2] }
sub min   { ($_[0]->_fields)[1] }
sub sec   { ($_[0]->_fields)[0] }
sub wday  { ($_[0]->_fields)[6] }
sub yday  { ($_[0]->_fields)[7] + 1 }
sub isdst { ($_[0]->_fields)[8] }

sub ymd { sprintf '%04d%s%02d%s%02d', $_[0]->year, $DATE_SEP, $_[0]->mon, $DATE_SEP, $_[0]->mday }
sub hms { sprintf '%02d%s%02d%s%02d', $_[0]->hour, $TIME_SEP, $_[0]->min, $TIME_SEP, $_[0]->sec }
sub datetime { $_[0]->ymd . 'T' . $_[0]->hms }
sub date { $_[0]->ymd }
sub time { $_[0]->hms }

sub strftime {
    my ($self, $fmt) = @_;
    my @t = $self->_fields;
    my %m = (
        year => $t[5] + 1900, mon => $t[4] + 1, mday => $t[3],
        hour => $t[2], min => $t[1], sec => $t[0],
        wday => $t[6], yday => $t[7] + 1, isdst => $t[8],
    );
    my $out = '';
    my $i = 0;
    my %pad = (Y => 4, m => 2, d => 2, H => 2, M => 2, S => 2, j => 3, y => 2, e => 2);
    while ($i < length $fmt) {
        my $c = substr($fmt, $i, 1);
        if ($c ne '%') { $out .= $c; $i++; next }
        my $k = substr($fmt, $i + 1, 1);
        $i += 2;
        if    ($k eq 'Y') { $out .= $m{year} }
        elsif ($k eq 'y') { $out .= sprintf '%02d', $m{year} % 100 }
        elsif ($k eq 'm') { $out .= sprintf '%02d', $m{mon} }
        elsif ($k eq 'd') { $out .= sprintf '%02d', $m{mday} }
        elsif ($k eq 'e') { $out .= sprintf '%2d',  $m{mday} }
        elsif ($k eq 'H') { $out .= sprintf '%02d', $m{hour} }
        elsif ($k eq 'M') { $out .= sprintf '%02d', $m{min} }
        elsif ($k eq 'S') { $out .= sprintf '%02d', $m{sec} }
        elsif ($k eq 'j') { $out .= sprintf '%03d', $m{yday} }
        elsif ($k eq 'b') { $out .= $MON_LIST[$m{mon} - 1] }
        elsif ($k eq 'B') { $out .= $FULLMON_LIST[$m{mon} - 1] }
        elsif ($k eq 'a') { $out .= $DAY_LIST[$m{wday}] }
        elsif ($k eq 'A') { $out .= $FULLDAY_LIST[$m{wday}] }
        elsif ($k eq 'F') { $out .= sprintf '%04d-%02d-%02d', $m{year}, $m{mon}, $m{mday} }
        elsif ($k eq 'T') { $out .= sprintf '%02d:%02d:%02d', $m{hour}, $m{min}, $m{sec} }
        elsif ($k eq 's') { $out .= $self->{_epoch} }
        elsif ($k eq '%') { $out .= '%' }
        else              { $out .= "%$k" }
    }
    return $out;
}

sub strptime {
    my ($class, $str, $fmt) = @_;
    $fmt = '%Y-%m-%dT%H:%M:%S' unless defined $fmt;
    my %v;
    my $re = '';
    my $i  = 0;
    while ($i < length $fmt) {
        my $c = substr($fmt, $i, 1);
        if ($c ne '%') { $re .= quotemeta $c; $i++; next }
        my $k = substr($fmt, $i + 1, 1);
        $i += 2;
        if    ($k eq 'Y') { $re .= '(?<Y>\d{4})' }
        elsif ($k eq 'y') { $re .= '(?<y>\d{2})' }
        elsif ($k eq 'm') { $re .= '(?<m>\d{1,2})' }
        elsif ($k eq 'd' or $k eq 'e') { $re .= '\s*(?<d>\d{1,2})' }
        elsif ($k eq 'H') { $re .= '(?<H>\d{1,2})' }
        elsif ($k eq 'M') { $re .= '(?<M>\d{1,2})' }
        elsif ($k eq 'S') { $re .= '(?<S>\d{1,2})' }
        elsif ($k eq 'j') { $re .= '(?<j>\d{1,3})' }
        elsif ($k eq 'b') { $re .= '(?<b>[A-Za-z]{3,})' }
        elsif ($k eq 'B') { $re .= '(?<b>[A-Za-z]+)' }
        elsif ($k eq 'a' or $k eq 'A') { $re .= '[A-Za-z]+' }
        elsif ($k eq 'T') { $re .= '(?<H>\d{1,2}):(?<M>\d{1,2}):(?<S>\d{1,2})' }
        elsif ($k eq 'F') { $re .= '(?<Y>\d{4})-(?<m>\d{1,2})-(?<d>\d{1,2})' }
        elsif ($k eq '%') { $re .= '%' }
        else { $re .= '.*?' }
    }
    $str =~ /^\s*$re\s*$/i
        or Carp::croak("Time::Piece::strptime: '$str' does not match '$fmt'");
    %v = %+;

    my ($year, $mon, $mday, $hour, $min, $sec);
    $year = $v{Y};
    $year = ($v{y} > 68 ? 1900 : 2000) + $v{y} if !defined $year && defined $v{y};
    $mon  = $v{m};
    if (!defined $mon && defined $v{b}) {
        $mon = $MON_NUM{lc $v{b}} || $FULLMON_NUM{lc $v{b}}
            or Carp::croak("Time::Piece::strptime: unknown month '$v{b}'");
    }
    $mday = $v{d};
    $hour = defined $v{H} ? $v{H} : 0;
    $min  = defined $v{M} ? $v{M} : 0;
    $sec  = defined $v{S} ? $v{S} : 0;
    $mon  = 1 unless defined $mon;
    $mday = 1 unless defined $mday;
    $year = 1970 unless defined $year;

    require Time::Local;
    my $epoch = Time::Local::timelocal($sec, $min, $hour, $mday, $mon - 1, $year);
    return bless { _epoch => $epoch, _islocal => 1 }, (ref($class) || $class || 'Time::Piece');
}

use overload
    '""'     => sub { $_[0]->strftime('%a %b %e %H:%M:%S %Y') },
    fallback => 1;

1;
